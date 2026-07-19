#[allow(unused)]
pub trait Control {
    fn next_sample(&mut self) -> f32;
    fn set_freq(&mut self, freq: f32);
    fn note_on(&mut self, vel: u8);
    fn note_off(&mut self);
    fn set_float_param(&mut self, key: u8, value: f32) {}
}

pub trait Next {
    fn update(&mut self);
    fn patch(&mut self) -> f32;
}
