use std::hash::{self, Hasher};

pub trait HashBuilderExt: Sized + hash::BuildHasher {
    fn hash_one<H: ?Sized + hash::Hash>(&self, target: &H) -> u64 {
        let mut hasher = self.build_hasher();
        target.hash(&mut hasher);
        hasher.finish()
    }
}

impl<T: hash::BuildHasher> HashBuilderExt for T {}
