// IronTrack - Open-source flight management and aerial survey planning engine
// Copyright (C) 2026 [Founder Name]
// SPDX-License-Identifier: GPL-3.0-or-later
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use crate::network::nmea::NmeaParser;
use crate::network::server::SharedState;
use serialport::{available_ports, SerialPortType, UsbPortInfo};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::sleep;
use tokio_serial::SerialStream;

pub struct SerialManager {
    vid: u16,
    pid: u16,
    state: SharedState,
}

impl SerialManager {
    pub fn new(vid: u16, pid: u16, state: SharedState) -> Self {
        Self { vid, pid, state }
    }

    pub async fn run(&self) {
        loop {
            if let Some(port_path) = self.find_port() {
                println!("SerialManager: Found target device at {}", port_path);
                if let Err(e) = self.read_loop(&port_path).await {
                    eprintln!("SerialManager error on {}: {}", port_path, e);
                }
            }

            // Wait before retry (hot-plug backoff)
            sleep(Duration::from_millis(500)).await;
        }
    }

    fn find_port(&self) -> Option<String> {
        let ports = available_ports().ok()?;
        for p in ports {
            if let SerialPortType::UsbPort(UsbPortInfo { vid, pid, .. }) = p.port_type {
                if vid == self.vid && pid == self.pid {
                    return Some(p.port_name);
                }
            }
        }
        None
    }

    async fn read_loop(&self, path: &str) -> anyhow::Result<()> {
        let builder = tokio_serial::new(path, 115_200); // Base baud, will be higher in production
        let stream = SerialStream::open(&builder)?;
        let mut reader = BufReader::new(stream);
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                return Err(anyhow::anyhow!("EOF reached"));
            }

            let mut current_telemetry = {
                let state_guard = self.state.read().await;
                state_guard.last_telemetry.clone()
            };

            if NmeaParser::parse(&line, &mut current_telemetry) {
                // Check for RTK degradation
                let old_quality = {
                    let state_guard = self.state.read().await;
                    state_guard.last_telemetry.fix_quality
                };

                if old_quality == 4 && current_telemetry.fix_quality < 4 {
                    eprintln!(
                        "WARNING: RTK degradation detected! Quality: {} -> {}",
                        old_quality, current_telemetry.fix_quality
                    );
                    // In v0.4+ this would broadcast an event. For now we just update state.
                }

                let mut state_guard = self.state.write().await;
                state_guard.last_telemetry = current_telemetry.clone();
                let _ = state_guard.tx_telemetry.send(current_telemetry);
            }
        }
    }
}
