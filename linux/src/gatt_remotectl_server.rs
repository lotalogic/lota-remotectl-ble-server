//! Serves a Bluetooth GATT application using the IO programming model.

use bluer::{
    adv::Advertisement,
    gatt::{
        local::{
            characteristic_control, service_control, Application, Characteristic, CharacteristicControlEvent,
            CharacteristicNotify, CharacteristicNotifyMethod, CharacteristicWrite, CharacteristicWriteMethod,
            Service,
        },
        CharacteristicReader, /*CharacteristicWriter,*/
    },
};
use futures::{future, pin_mut, StreamExt};
use std::{collections::BTreeMap, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, /*AsyncWriteExt,*/ BufReader},
    time::{sleep},
};
use std::process::Command;
use std::str;

mod check_environment;
use check_environment::check_environment;

include!("gatt.inc");

const IR_CTL_CMD: &str = "ir-ctl";

fn process_command_if_complete(buffer: &mut String) {
    println!("Entering process_command_if_complete");

    if let Some(end_index) = buffer.find("</dummycmd>") {
        let start_index = buffer.find("<dummycmd>").unwrap() + "<dummycmd>".len();
        let command_text = &buffer[start_index..end_index];

        println!("command_text: {}", command_text);

        if let Some(colon_index) = command_text.find(':') {
            let protocol = &command_text[..colon_index];
            let scancode = &command_text[colon_index + 1..];

            // Start building the command with basic arguments
            let mut command_args = vec!["-v", "-d", "/dev/lirc-tx"];

            // Adjust command arguments based on protocol
            if protocol.starts_with("sony") {
                command_args.extend_from_slice(&["-g", "600"]);
            }

            // Append the "-S protocol:scancode" argument(s)
            let protocol_scancode_arg = format!("{}:{}", protocol, scancode);
            command_args.push("-S");
            command_args.push(&protocol_scancode_arg);

            // If the protocol is "sony", repeat the "-S protocol:scancode" part two more times
            if protocol.starts_with("sony") {
                command_args.push("-S");
                command_args.push(&protocol_scancode_arg);
                command_args.push("-S");
                command_args.push(&protocol_scancode_arg);
            }

            println!("Executing command: {} {}", IR_CTL_CMD, command_args.join(" "));

            // Execute ir-ctl command with the prepared arguments
            let output = Command::new(IR_CTL_CMD)
                .args(&command_args)
                .output()
                .expect("Failed to execute command");

            println!("Command executed, output: {:?}", str::from_utf8(&output.stdout));

            // Clear the buffer after processing
            buffer.clear();
        }
    }
}


#[tokio::main(flavor = "current_thread")]
async fn main() -> bluer::Result<()> {
    env_logger::init();

    if !check_environment().await? {
        return Ok(());
    }


    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;

    println!("Advertising on Bluetooth adapter {} with address {}", adapter.name(), adapter.address().await?);
    let mut manufacturer_data = BTreeMap::new();
    manufacturer_data.insert(MANUFACTURER_ID, vec![0x21, 0x22, 0x23, 0x24]);
    let le_advertisement = Advertisement {
        service_uuids: vec![SERVICE_UUID].into_iter().collect(),
        manufacturer_data,
        discoverable: Some(true),
        local_name: Some("gatt_server".to_string()),
        ..Default::default()
    };
    let adv_handle = adapter.advertise(le_advertisement).await?;

    println!("Serving GATT service on Bluetooth adapter {}", adapter.name());
    let (service_control, service_handle) = service_control();
    let (char_control, char_handle) = characteristic_control();
    let app = Application {
        services: vec![Service {
            uuid: SERVICE_UUID,
            primary: true,
            characteristics: vec![Characteristic {
                uuid: CHARACTERISTIC_UUID,
                write: Some(CharacteristicWrite {
                    write: true,
                    write_without_response: true,
                    method: CharacteristicWriteMethod::Io,
                    ..Default::default()
                }),
                notify: Some(CharacteristicNotify {
                    notify: true,
                    method: CharacteristicNotifyMethod::Io,
                    ..Default::default()
                }),
                control_handle: char_handle,
                ..Default::default()
            }],
            control_handle: service_handle,
            ..Default::default()
        }],
        ..Default::default()
    };
    let app_handle = adapter.serve_gatt_application(app).await?;

    println!("Service handle is 0x{:x}", service_control.handle()?);
    println!("Characteristic handle is 0x{:x}", char_control.handle()?);

    println!("Service ready. Press enter to quit.");
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    //let mut value: Vec<u8> = vec![0x00];
    let mut read_buf = Vec::new();
    let mut reader_opt: Option<CharacteristicReader> = None;
    //let mut writer_opt: Option<CharacteristicWriter> = None;
    let mut buffer = String::new();
    pin_mut!(char_control);

    loop {
        tokio::select! {
            _ = lines.next_line() => break,
            evt = char_control.next() => {
                match evt {
                    Some(CharacteristicControlEvent::Write(req)) => {
                        println!("Accepting write event with MTU {} from {}", req.mtu(), req.device_address());
                        read_buf = vec![0; req.mtu()];
                        reader_opt = Some(req.accept()?);
                    },
                    Some(CharacteristicControlEvent::Notify(notifier)) => {
                        println!("Accepting notify request event with MTU {} from {}", notifier.mtu(), notifier.device_address());
                        //writer_opt = Some(notifier);
                    },
                    None => break,
                }
            }
            read_res = async {
                match &mut reader_opt {
                    Some(reader) => reader.read(&mut read_buf).await,
                    None => future::pending().await,
                }
            } => {
                match read_res {
                    Ok(0) => {
                        println!("Write stream ended");
                        reader_opt = None;
                    }
                    Ok(n) => {
                        let value = read_buf[0..n].to_vec();
                        println!("Write request with {} bytes: {:x?}", n, &value);
                        match String::from_utf8(value.clone()) {
                            Ok(text) => {
                                println!("Received text: {}", text);
                                buffer.push_str(&text);
                                process_command_if_complete(&mut buffer);
                            }
                            Err(e) => {
                                println!("Failed to decode received bytes as UTF-8. Error: {}", e);
                                // Log the bytes in hexadecimal format if decoding fails
                                println!("Received bytes: {:x?}", &value);
                            }
                        }
                    }
                    Err(err) => {
                        println!("Write stream error: {}", &err);
                        reader_opt = None;
                    }
                }
            }
        }
    }

    println!("Removing service and advertisement");
    drop(app_handle);
    drop(adv_handle);
    sleep(Duration::from_secs(1)).await;

    Ok(())
}
