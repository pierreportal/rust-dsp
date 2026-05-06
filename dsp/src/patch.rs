pub trait Module {
    fn process(&mut self, input: f32) -> f32;
}

#[macro_export]
macro_rules! patch {
    ($($module:expr)=>+ $(,)?) => {{
        move |mut input| {
            $(
                input = $module.process(input);
            )+
            input
        }
    }};
}
