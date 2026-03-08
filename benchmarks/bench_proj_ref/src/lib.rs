pub struct Record {
    pub name: String,
    pub id: u32,
}
impl Record {
    pub fn new(name: String, id: u32) -> Self {
        Record { name, id }
    }
    pub fn name_ref(&self) -> &String {
        &self.name
    }
    pub fn id_val(&self) -> u32 {
        self.id
    }
}
pub fn string_len(s: &String) -> usize {
    s.len()
}
