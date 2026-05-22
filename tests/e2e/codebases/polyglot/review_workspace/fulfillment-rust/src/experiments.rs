pub struct DroneDispatchPlan {
    pub launch_pad: String,
    pub delivery_radius_miles: u32,
}

pub fn plan_drone_dispatch(zip_code: &str) -> DroneDispatchPlan {
    DroneDispatchPlan {
        launch_pad: format!("pad-{zip_code}"),
        delivery_radius_miles: 12,
    }
}

fn legacy_zip_zone(zip_code: &str) -> String {
    zip_code.chars().take(3).collect()
}
