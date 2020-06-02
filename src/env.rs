pub struct Env{
    pub is_shell:bool
}
pub static mut IonEnv: Env=Env{
    is_shell:false,
};
