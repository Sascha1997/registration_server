// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use config::Config;
use domain_store::SqlParam;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// We evict the local_ip info from records older than EVICTION_DELAY.
// Clients should renew their registration at a shorter interval.

pub fn evict_old_entries(config: &Config) {
    let delay = config.eviction_delay;
    let db = config.domain_db.clone();
    thread::Builder::new()
        .name("eviction".into())
        .spawn(move || {
            info!("Starting eviction thread, delay is {}s", delay);
            loop {
                thread::sleep_ms(delay * 1000);
                let max_age = (SystemTime::now().duration_since(UNIX_EPOCH).unwrap() -
                               Duration::new(delay as u64, 0))
                        .as_secs();
                info!("Checking for records older than {}", max_age);
                match db.execute_1param_sql("DELETE FROM domains WHERE timestamp<$1",
                                            SqlParam::Integer(max_age as i64))
                          .recv()
                          .unwrap() {
                    Err(err) => error!("Error evicting old records: {:?}", err),
                    Ok(count) => info!("Evicted {} records.", count),
                }
            }
        });
}