use bevy_ecs::resource::Resource;
use std::{fmt, str::FromStr};

pub type Key = Box<[u8]>;

pub struct KeyDisplay<'a>(pub &'a [u8]);

impl fmt::Display for KeyDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        String::from_utf8_lossy(self.0).fmt(f)
    }
}
#[derive(Resource)]
pub struct DefKey<const N: usize = 0>(pub Key);

impl<const N: usize> fmt::Debug for DefKey<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("DefKey").field(&self.0).finish()
    }
}

impl<const N: usize> fmt::Display for DefKey<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        String::from_utf8_lossy(&self.0).fmt(f)
    }
}

impl<const N: usize> Clone for DefKey<N> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<const N: usize> FromStr for DefKey<N> {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_bytes(s.as_bytes()))
    }
}

impl<const N: usize> DefKey<N> {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(bytes.into())
    }

    pub fn arg<const M: usize>(&self) -> DefKey<M> {
        DefKey(self.0.clone())
    }

    pub fn key(&self) -> Key {
        self.0.clone()
    }
}

impl<const N: usize> std::ops::Deref for DefKey<N> {
    type Target = Key;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
