#![cfg(feature = "glib-runtime")]
use glib::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, glib::Enum)]
#[enum_type(name = "AbbeyCompatWitnessEnum")]
#[repr(i32)]
enum Witness {
    First = 7,
    Second = 31,
}

#[test]
fn enum_value_and_clone_roundtrip() {
    for value in [Witness::First, Witness::Second] {
        let stored = value.to_value();
        assert_eq!(stored.type_(), Witness::static_type());
        assert_eq!(stored.get::<Witness>().unwrap(), value);
    }
    assert_eq!(Witness::static_type().name(), "AbbeyCompatWitnessEnum");
    let class = glib::EnumClass::new::<Witness>();
    assert_eq!(
        class
            .values()
            .iter()
            .map(|value| value.value())
            .collect::<Vec<_>>(),
        vec![7, 31]
    );
    assert!(class.value(8).is_none());
    let label = std::rc::Rc::new(String::from("Abbey clone witness"));
    let closure = glib::clone!(@strong label => move || label.to_string());
    assert_eq!(closure(), "Abbey clone witness");
}

#[test]
fn optimized_variant_string_iteration() {
    for values in [vec![], vec!["only"], vec!["a", "雪", "c", "🦀", "tail"]] {
        let variant = values.to_variant();
        assert_eq!(
            variant.array_iter_str().unwrap().collect::<Vec<_>>(),
            values
        );
        assert_eq!(
            variant.array_iter_str().unwrap().rev().collect::<Vec<_>>(),
            values.iter().rev().copied().collect::<Vec<_>>()
        );
        for skip in 0..=values.len() {
            assert_eq!(
                variant.array_iter_str().unwrap().nth(skip),
                values.get(skip).copied()
            );
            assert_eq!(
                variant.array_iter_str().unwrap().nth_back(skip),
                values.iter().rev().nth(skip).copied()
            );
        }
        assert_eq!(
            variant.array_iter_str().unwrap().last(),
            values.last().copied()
        );
        let mut mixed = variant.array_iter_str().unwrap();
        let mut expected = std::collections::VecDeque::from(values);
        while !expected.is_empty() {
            assert_eq!(mixed.next(), expected.pop_front());
            if !expected.is_empty() {
                assert_eq!(mixed.next_back(), expected.pop_back());
            }
        }
        assert_eq!(mixed.next(), None);
        assert_eq!(mixed.next_back(), None);
    }
}
