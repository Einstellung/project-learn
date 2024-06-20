use crate::{builder::CircuitBuilder, CompiledCircuit};
use std::ops::{Add, Mul};

pub trait CircuitDescription<const INPUTS: usize>: Sized {
    fn run<V: Var>(inputs: [V; INPUTS]);
    fn build() -> CompiledCircuit<INPUTS, Self> {
        CircuitBuilder::compile::<INPUTS, Self>()
    }
}

pub trait Var
where
    Self: Sized + Add<Output = Self> + Mul<Output = Self> + Clone,
{
    fn assert_eq(&self, other: &Self);
}

pub trait VariableTrait
where
    Self: Sized + Add<Output = Self> + Mul<Output = Self>,
{
}

#[cfg(test)]
mod tests {

    struct MyStruct<T> {
        data: i32,
        _maker: std::marker::PhantomData<T>,
        // There is no field of type `T` here
    }
    
    impl<T> MyStruct<T> {
        fn new(data: i32) -> Self {
            MyStruct { data, 
            _maker: std::marker::PhantomData }
        }
    }
    
    #[test]
    fn main() {
        // We can create MyStruct with any type, but the type is not stored
        let my_struct: MyStruct<String> = MyStruct::new(10);
        println!("Data: {}", my_struct.data);
    }
    
}
