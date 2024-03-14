use std::process::{Command, Stdio};
use std::io::{self};
use std::collections::HashMap;
use regex::Regex;

//////////////////////////////////////////////////////////////////////////
// TODO: Rewrite all this code by calling kernel functions directly
// and not use buggy ir-keytable
//////////////////////////////////////////////////////////////////////////


// Function to check if a binary is installed
fn is_binary_installed(binary_name: &str) -> bool {
    Command::new("which")
        .arg(binary_name)
        .stdout(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

// Function to execute a command and return its stderr (since buggy ir-keytable uses that!!!)
fn exec_get_stderr(command: &str, args: &[&str]) -> io::Result<String> {
    let output = Command::new(command)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;

    let error_str = String::from_utf8_lossy(&output.stderr).to_string();

    Ok(error_str)
}

// Function to parse the output of `ir-keytable` into a structured format
fn parse_ir_keytable_output(output: &str) -> Vec<HashMap<String, String>> {
    let mut sections: Vec<HashMap<String, String>> = Vec::new();
    let mut current_section = HashMap::new();
    let re = Regex::new(r"^Found (.+?) with:$").unwrap();
    let lines = output.lines();

    fn contains_lirc_key(section: &HashMap<String, String>) -> bool {
        section.keys().any(|k| k.contains("LIRC"))
    }

    for line in lines {
        if let Some(caps) = re.captures(line) {
            if !current_section.is_empty() && contains_lirc_key(&current_section) {
                sections.push(current_section.clone());
                current_section.clear();
            }
            let sysdev_path = caps.get(1).map_or("", |m| m.as_str());
            current_section.insert("__sysdev__".to_string(), sysdev_path.to_string());
        }
        // Split line by comma for multiple key-value pairs
        let pairs = line.split(',');
        for pair in pairs {
            let parts: Vec<&str> = pair.splitn(2, ':').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_string();
                let value = parts[1].trim().to_string();
                current_section.insert(key, value);
            }
        }
    }

    // Don't forget to add the last section if it exists
    if !current_section.is_empty() && contains_lirc_key(&current_section) {
        sections.push(current_section);
    }

    sections
}


pub async fn check_environment() -> io::Result<bool> {
    for binary in &["ir-keytable", "ir-ctl"] {
        if !is_binary_installed(binary) {
            println!("{} is not installed. Please install {} (e.g., sudo apt install v4l-utils).", binary, binary);
            return Ok(false);
        }
    }

    let ir_keytable_stderr = exec_get_stderr("ir-keytable", &[])?;
    if ir_keytable_stderr.is_empty() {
        println!("We cannot find any infrared device.");
        return Ok(false);
    }

    let sections = parse_ir_keytable_output(&ir_keytable_stderr);
    for section in sections {
        println!("{:?}", section);
        //TODO: warn 
    }
    

    Ok(true)
}

