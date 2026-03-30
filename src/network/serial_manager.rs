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
use crate::network::telemetry::ServerMsg;
use serialport::{available_ports, SerialPortType, UsbPortInfo};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::sleep;
use tokio_serial::SerialStream;

/// Consecutive degraded epochs before broadcasting an RTK warning.
/// At 10 Hz, 20 epochs = 2 seconds of sustained degradation.
const RTK_DEBOUNCE_EPOCHS: u16 = 20;

/// If no valid NMEA sentence is parsed within this duration, assume
/// the serial link is dead and trigger hot-plug recovery.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(3);

pub struct SerialManager {
    vid: u16,
    pid: u16,
    baud_rate: u32,
    state: SharedState,
    start: Instant,
}

impl SerialManager {
    pub fn new(vid: u16, pid: u16, baud_rate: u32, state: SharedState) -> Self {
        Self {
            vid,
            pid,
            baud_rate,
            state,
            start: Instant::now(),
        }
    }

    pub async fn run(&self) {
        loop {
            if let Some(port_path) = self.find_port() {
                log::info!("SerialManager: found target device at {}", port_path);
                if let Err(e) = self.read_loop(&port_path).await {
                    log::warn!("SerialManager error on {}: {}", port_path, e);
                }
            }

            // Hot-plug backoff (Step 2 of 5-step recovery) [Doc 29 §4.3]
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
        let builder = tokio_serial::new(path, self.baud_rate);
        let stream = SerialStream::open(&builder)?;
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let mut parser = NmeaParser::new();
        let mut consecutive_degraded: u16 = 0;

        loop {
            line.clear();

            // Heartbeat watchdog: if no line received within HEARTBEAT_TIMEOUT,
            // the serial link is presumed dead. Break out to trigger hot-plug
            // recovery. Wrapping read_line in tokio::time::timeout handles the
            // case where the GNSS stops transmitting without closing the port.
            let read_result = tokio::time::timeout(HEARTBEAT_TIMEOUT, reader.read_line(&mut line))
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "heartbeat timeout: no serial data for {}s",
                        HEARTBEAT_TIMEOUT.as_secs()
                    )
                })?;
            let bytes_read = read_result?;
            if bytes_read == 0 {
                return Err(anyhow::anyhow!("EOF reached"));
            }

            let mut current_telemetry = {
                let state_guard = self.state.read().await;
                state_guard.last_telemetry.clone()
            };

            if parser.parse(&line, &mut current_telemetry) {
                // RTK degradation detection with debounce [Doc 23 §3.3]
                let old_quality = {
                    let state_guard = self.state.read().await;
                    state_guard.last_telemetry.fix_quality
                };

                if current_telemetry.fix_quality < 4 && old_quality >= 4 {
                    // Degradation edge detected — start counting
                    consecutive_degraded = 1;
                } else if current_telemetry.fix_quality < 4 {
                    consecutive_degraded = consecutive_degraded.saturating_add(1);
                } else {
                    consecutive_degraded = 0;
                }

                // Broadcast warning only after sustained degradation
                if consecutive_degraded == RTK_DEBOUNCE_EPOCHS {
                    log::warn!(
                        "RTK degradation sustained for {} epochs — quality: {} → {}",
                        RTK_DEBOUNCE_EPOCHS,
                        old_quality,
                        current_telemetry.fix_quality
                    );
                    let warning = ServerMsg::Warning {
                        code: "RTK_DEGRADATION".to_string(),
                        message: format!(
                            "RTK fix quality degraded from {} to {} (sustained {} epochs)",
                            old_quality, current_telemetry.fix_quality, RTK_DEBOUNCE_EPOCHS,
                        ),
                        timestamp_ms: self.start.elapsed().as_millis() as u64,
                    };
                    let state_guard = self.state.read().await;
                    let _ = state_guard.tx_warning.send(warning);
                }

                let mut state_guard = self.state.write().await;
                state_guard.last_telemetry = current_telemetry.clone();
                let _ = state_guard.tx_telemetry.send(current_telemetry);
            }
        }
    }
}
