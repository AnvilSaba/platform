use super::ports::LinkCodeGenerator;
use rand::RngExt;

const CODE_LENGTH: usize = 8;
const CODE_CHARACTERS: &[u8] = b"ACDEFGHJKMNPQRTUVWXYZacdefghjkmnpqrtuvwxyz234679";

#[derive(Clone)]
pub struct RandomLinkCodeGenerator;

impl LinkCodeGenerator for RandomLinkCodeGenerator {
    fn generate(&self) -> String {
        let mut rng = rand::rng();
        (0..CODE_LENGTH)
            .map(|_| CODE_CHARACTERS[rng.random_range(0..CODE_CHARACTERS.len())] as char)
            .collect()
    }
}
