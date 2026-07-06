use std::time::Duration;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::wasm_bindgen;
use worker::{
    console_error, console_log, durable_object, event, Context, Date, DurableObject, Env, Error,
    Headers, Request, Response, State,
};

#[event(fetch)]
async fn fetch(_req: Request, _env: Env, _ctx: Context) -> Result<Response, Error> {
    match _env.durable_object("RUSTY_LIMITER") {
        Ok(namespace) => {
            let stub = namespace
                .id_from_name("test-id")
                .unwrap()
                .get_stub()
                .unwrap();

            return stub.fetch_with_request(_req).await;
        }
        Err(err) => console_error!("Error getting binding for DO {}", err),
    }
    Response::ok("Hello World!")
}

#[durable_object]
pub struct RustyLimiter {
    // next_allowed_time: i64,
    // in_cooldown: bool,
    // sql: SqlStorage,
    kv_key: &'static str,
    state: State,
    env: Env,
}

#[wasm_bindgen]
impl RustyLimiter {
    pub fn test(&mut self) {
        // if self.in_cooldown {
        //     console_log!("in cooldown");
        // } else {
        //     console_log!("not in cooldown");
        // }
        //
        // self.in_cooldown = !self.in_cooldown;
    }
}

#[derive(Deserialize, Serialize)]
struct RateLimitRows {
    in_cooldown: bool,
    next_allowed_time: u64,
    counter: u32,
}

const MAX_REQ_SEC: u32 = 100;

impl DurableObject for RustyLimiter {
    fn new(state: State, env: Env) -> Self {
        Self {
            kv_key: "rate-limit",
            state: state,
            env: env,
        }
    }

    async fn alarm(&self) -> Result<Response, Error> {
        let _ = self.state.storage().delete(self.kv_key).await;
        console_log!("reseting rate-limit");
        Response::empty()
    }

    // alarm will reset the counter
    async fn fetch(&self, _req: Request) -> Result<Response, Error> {
        let _ = self.state.storage().delete_alarm().await;
        match self.state.storage().get_alarm().await {
            Ok(alarm) => match alarm {
                Some(_) => {
                    console_log!("alarm already set");
                }
                None => {
                    let storage = self.state.storage();
                    self.state.wait_until(async move {
                        let sch = Duration::from_millis(Date::now().as_millis() + 60 * 1000);
                        let _ = storage.set_alarm(sch).await;

                        console_log!("created alarm {}", sch.as_millis());
                    });
                }
            },
            Err(e) => console_error!("error retrieving alarm {}", e),
        }

        match self.state.storage().get::<RateLimitRows>(self.kv_key).await {
            Ok(entry) => match entry {
                Some(mut val) => {
                    val.counter += 1;

                    console_log!("counter {}", val.counter);
                    let now = Date::now();
                    let now_millis = now.as_millis();
                    if val.counter >= MAX_REQ_SEC
                        && val.next_allowed_time != 0
                        && val.next_allowed_time < now_millis
                    {
                        val.next_allowed_time =
                            Duration::from_secs(60).as_secs() * 1000 + now_millis;
                        let storage = self.state.storage();
                        let kv_key = self.kv_key;
                        self.state.wait_until(async move {
                            let _ = storage.put(kv_key, &val).await;
                        });
                    } else if val.next_allowed_time >= now_millis {
                        console_log!("rate limited: {}", self.state.id().to_string());
                        let headers = Headers::new();
                        headers.set(
                            "Retry-After",
                            &(val.next_allowed_time - now_millis).to_string(),
                        )?;

                        let mut response = Response::error("Rate limited", 429)?;
                        response = response.with_headers(headers);

                        let _ = Result::<&Response, ()>::Ok(&response);
                    } else {
                        let storage = self.state.storage();
                        let kv_key = self.kv_key;
                        self.state.wait_until(async move {
                            let _ = storage.put(kv_key, &val).await;
                        });
                    }
                }
                None => {
                    let val = RateLimitRows {
                        next_allowed_time: 0,
                        counter: 1,
                        in_cooldown: false,
                    };

                    let storage = self.state.storage();
                    let kv_key = self.kv_key;
                    self.state.wait_until(async move {
                        let _ = storage.put(kv_key, &val).await;
                    });
                }
            },
            Err(_) => {}
        }
        Response::empty()
    }
}
