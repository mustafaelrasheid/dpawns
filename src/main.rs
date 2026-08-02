mod args;
mod filesystems;
mod utils;

use std::fs::{
    read_to_string,
    read,
    read_dir,
    set_permissions,
    create_dir_all,
};
use std::env::set_var;
use std::process::{Command, Child};
use std::ffi::CString;
use std::os::unix::prelude::PermissionsExt;
use nix::sys::reboot::{reboot, RebootMode};
use nix::NixPath;
use signal_hook::iterator::Signals;
use libc::{sethostname, SIGPWR, SIGTERM, SIGINT, SIGKILL, kill, sync};
use clap::Parser;
use crate::args::Cli;
use crate::utils::delay;

fn set_hostname(default_hostname: &str) {
    let hostname = read_to_string("/etc/hostname")
        .unwrap_or_else(|_e| { String::from(default_hostname) });
    let c_hostname = CString::new(&*hostname)
        .expect("Invalid hostname");

    unsafe {
        if sethostname(c_hostname.as_ptr(), c_hostname.len()) == -1 {
            eprintln!("Failed to set host name to {}", hostname);
        }
    }
}

fn set_vars(vars: &[[&str; 2]]) {
    for var in vars{
    	unsafe {
        	set_var(var[0], var[1]);
        }
    }
}

fn reload_udev() {
    Command::new("udevadm")
        .arg("hwdb")
        .arg("--update")
        .spawn()
        .and_then(|mut child| child.wait())
        .map_err(|e| {
            eprintln!("Failed to update udev hardware database: {}", e);
        })
        .ok()
        .and_then(|status| {
            if !status.success() {
                eprintln!(
                    "udevadm hwdb update exited with status code: {}",
                    status.code().unwrap_or(-1));
            }
            
            return Some(());
        });

    Command::new("udevadm")
        .arg("trigger")
        .spawn()
        .and_then(|mut child| child.wait())
        .map_err(|e| {
            eprintln!("Failed to trigger udev change refresh: {}", e);
        })
        .ok()
        .and_then(|status| {
            if !status.success() {
                eprintln!(
                    "udevadm trigger refresh command exited with code: {}",
                    status.code().unwrap());
            }
            
            return Some(());
        });
}

fn init_udev() -> Option<Child> {
    let udev_proc = Command::new("udevd")
        .arg("-d")
        .spawn()
        .map_err(|e| {
            eprintln!("Failed to start udev: {}", e);
        })
        .ok();
    
    reload_udev();

    return udev_proc;
}

fn init_main_proc() -> Option<Child> {
   return Command::new("login")
        .arg("/sbin/entry")
        .arg("--set-xdg-runtime-dir")
        .spawn()
        .map_err(|e| {
            eprintln!("Failed to run /sbin/login with /sbin/entry: {}", e);
        })
        .ok();
}

fn init_seatd() -> Option<Child> {
    return Command::new("seatd")
        .arg("-l")
        .arg("silent")
        .spawn()
        .and_then(|proc| {
            delay();
            set_permissions(
                "/run/seatd.sock",
                PermissionsExt::from_mode(0o666)
            ).unwrap_or_else(|e| {
                eprintln!("Failed to chmod seatd.sock: {}", e);
            });
            return Ok(proc);
        })
        .map_err(|e| {
            eprintln!("Failed to start seatd: {}", e);
        })
        .ok();
}

fn init_dbus() -> Option<Child> {
    const SOCKET_ADDR: &str = "/run/dbus/system_bus_socket";

    create_dir_all("/run/dbus").unwrap_or_else(|e| {
        eprintln!("Failed to create a run dir for dbus: {}", e);
    });

    return Command::new("dbus-daemon")
        .arg(&format!("--address=unix:path={}", SOCKET_ADDR))
        .arg("--system")
        .arg("--nofork")
        .spawn()
        .map_err(|e| {
            eprintln!("Failed to start dbus-daemon: {}", e);
        })
        .ok();
}

fn init_dhcpcd() -> Option<Child> {
    create_dir_all("/run/dhcp").unwrap_or_else(|e| {
        eprintln!("Failed to create db dir for dhcpcd: {}", e);
    });

    return Command::new("dhcpcd")
        .arg("-qq")
        .spawn()
        .map_err(|e| {
            eprintln!("Failed to start dhcpcd: {}", e);
        })
        .ok();
}

fn kill_all_procs(processes: &mut [Option<Child>]){
    for process in processes {
        if let Some(proc) = process {
            proc.kill().unwrap_or_else(|e| {
                eprintln!("Failed to kill process: {}", e);
            });
            proc.wait().and_then(|status| {
                if !status.success() {
                    eprintln!(
                        "process exited with error code {}",
                        status.code().unwrap_or(-1));
                }
                
                return Ok(());
            }).unwrap_or_else(|e| {
                eprintln!("Failed to wait for process: {}", e);
            });
        }
    }
}

fn kill_remaining_procs() {
    let processes_ids: Vec<u32> = read_dir("/proc").ok().and_then(|entries| {
        let mut buffer: Vec<u32> = Vec::new();

        for entry_or_err in entries {
            let entry = if let Ok(val) = entry_or_err {
                val
            } else { continue; };
            let path = entry.path();
            let filename = if let Some(val) = path.file_name() {
                val.to_string_lossy()
            } else { continue; };
            let pid = if let Ok(val) = filename.parse::<u32>() {
                val
            } else { continue; };
            buffer.push(pid);
        }
        
        return Some(buffer);
    }).unwrap_or(Vec::new());
    
    for proc_id in processes_ids  {
        if proc_id == 1 {
            continue;
        }
        unsafe {
            let _ = kill(
                proc_id as i32,
                SIGKILL);
        }
    }
}

fn init_signals() -> Option<Signals> {
    return Signals::new(&[SIGPWR, SIGTERM, SIGINT]).map_err(|e| {
        eprintln!("Failed to setup signals due to {}", e);
    }).ok();
}

fn recieved_shutdown_sig(signals: &mut Option<Signals>) -> bool {
    if let Some(signals) = signals {
        for sig in signals.pending() {
            match sig {
                SIGPWR | SIGTERM | SIGINT => {
                    return true;
                }
                _ => ()
            }
        }
    }
    
    return false;
}

fn get_timezone() -> String {
    return read("/etc/localtime")
        .ok()
        .and_then(|bytes| {
            return Some((
                bytes.clone(),
                bytes.iter().rposition(|&b| b == b'\n')?
            )) 
        })
        .and_then(|(bytes, last_pos)| {
            let before_last = &bytes[..last_pos];
            let tz_bytes = if let Some(second_last_pos) = before_last
                .iter().rposition(|&b| b == b'\n') {
                &bytes[second_last_pos + 1..last_pos]
            } else {
                &bytes[..last_pos]
            };

            return Some(std::str::from_utf8(tz_bytes).ok()?.to_string());
        })
        .unwrap_or(String::from("UTC"));
}

fn start() {
    let timezone = get_timezone();
    let vars = [
        ["PATH",  "/sbin:/usr/bin"],
        ["SHELL", "/usr/bin/bash"],
        ["TZ",    &timezone]
    ];

    filesystems::setup();
    set_vars(&vars);
    set_hostname("castle");
}

fn shutdown(processes: &mut [Option<Child>]){
    kill_all_procs(processes);
    delay();
    kill_remaining_procs();
    delay();
    filesystems::close();
    delay();
    unsafe {
        sync();
    }
    #[allow(unreachable_code)]{
        reboot(RebootMode::RB_POWER_OFF)
            .unwrap_or_else(|e| {
                eprintln!("Failed to reboot: {}", e);
                panic!();
            });
    }
}

fn main() {
    let _ = Cli::parse();
    let mut signals = init_signals();
    let mut processes: Vec<Option<Child>> = Vec::new();
    
    start();
    processes.push(init_udev());
    processes.push(init_seatd());
    processes.push(init_dbus());
    processes.push(init_dhcpcd());
    processes.push(init_main_proc());

    #[allow(unreachable_code)]
    loop {
        delay();
        if recieved_shutdown_sig(&mut signals){
            shutdown(&mut processes);
        }
    }
}
