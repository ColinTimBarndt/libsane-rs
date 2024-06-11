use core::fmt;
use std::ffi::c_void;

use bitflags::bitflags;

use crate::{
    list::{new_word_list, SaneStrListIter},
    sys, ControlInfo, DeviceHandle, Error, Fixed, OwnedValue, SaneStr, SaneString, Value,
    ValueType, WithSane,
};

use super::RawDeviceHandle;

#[derive(Clone, Copy)]
pub struct DeviceOption<'a, S: WithSane> {
    raw: &'a RawDeviceHandle<S>,
    descriptor: *const sys::OptionDescriptor,
    index: u32,
}

impl<'a, S: WithSane> DeviceOption<'a, S> {
    pub(super) unsafe fn new(
        raw: &'a RawDeviceHandle<S>,
        descriptor: *const sys::OptionDescriptor,
        index: u32,
    ) -> Self {
        Self {
            raw,
            descriptor,
            index,
        }
    }

    pub fn name(&self) -> &SaneStr {
        self.raw
            // SAFETY: reading is synchronized, and the device has not been closed. By spec, this is a valid C-String.
            .with_sane(|_| unsafe { SaneStr::from_ptr((*self.descriptor).name) })
    }

    pub fn title(&self) -> &SaneStr {
        self.raw
            // SAFETY: reading is synchronized, and the device has not been closed. By spec, this is a valid C-String.
            .with_sane(|_| unsafe { SaneStr::from_ptr((*self.descriptor).name) })
    }

    pub fn description(&self) -> &SaneStr {
        self.raw
            // SAFETY: reading is synchronized, and the device has not been closed. By spec, this is a valid C-String.
            .with_sane(|_| unsafe { SaneStr::from_ptr((*self.descriptor).desc) })
    }

    pub fn type_(&self) -> ValueType {
        ValueType::from(self.sys_type())
    }

    pub fn sys_type(&self) -> sys::ValueType {
        // SAFETY: reading is synchronized, and the device has not been closed.
        self.raw.with_sane(|_| unsafe { (*self.descriptor).type_ })
    }

    pub fn unit(&self) -> sys::Unit {
        // SAFETY: reading is synchronized, and the device has not been closed.
        self.raw.with_sane(|_| unsafe { (*self.descriptor).unit })
    }

    pub fn size(&self) -> usize {
        self.raw
            // SAFETY: reading is synchronized, and the device has not been closed.
            .with_sane(|_| unsafe { (*self.descriptor).size as usize })
    }

    pub fn capabilities(&self) -> DeviceOptionCapabilities {
        self.raw.with_sane(|_| {
            // SAFETY: reading is synchronized, and the device has not been closed.
            DeviceOptionCapabilities::from_bits_retain(unsafe { (*self.descriptor).cap } as u32)
        })
    }

    pub fn constraint(&self) -> Option<DeviceOptionConstraint> {
        self.raw.with_sane(|_| {
            // SAFETY: reading is synchronized, and the device has not been closed.
            let ctype = unsafe { (*self.descriptor).constraint_type };
            // SAFETY: reading is synchronized, and the device has not been closed.
            let value_type = unsafe { (*self.descriptor).type_ };
            match ctype {
                sys::ConstraintType::None => None,
                sys::ConstraintType::Range => Some({
                    // SAFETY: By the spec, the union has a value of range
                    let r = unsafe { &*(*self.descriptor).constraint.range };
                    match value_type {
                        sys::ValueType::Int => DeviceOptionConstraint::RangeInt {
                            min: r.min,
                            max: r.max,
                            quant: r.quant,
                        },
                        sys::ValueType::Fixed => DeviceOptionConstraint::RangeFixed {
                            min: Fixed::from_bits(r.min),
                            max: Fixed::from_bits(r.max),
                            quant: Fixed::from_bits(r.quant),
                        },
                        other => DeviceOptionConstraint::Unsupported {
                            value_type: other,
                            contraint_type: sys::ConstraintType::Range,
                        },
                    }
                }),
                sys::ConstraintType::WordList => Some({
                    // SAFETY: By the spec, the union has a value of word_list
                    let data = unsafe { (*self.descriptor).constraint.word_list };
                    match value_type {
                        sys::ValueType::Int => {
                            DeviceOptionConstraint::ListInt(
                                // SAFETY: by spec https://sane-project.gitlab.io/standard/api.html#option-value-constraints
                                unsafe { new_word_list::<i32>(data) },
                            )
                        }
                        sys::ValueType::Fixed => DeviceOptionConstraint::ListFixed(
                            // SAFETY: by spec https://sane-project.gitlab.io/standard/api.html#option-value-constraints
                            unsafe { new_word_list::<Fixed>(data) },
                        ),
                        other => DeviceOptionConstraint::Unsupported {
                            value_type: other,
                            contraint_type: sys::ConstraintType::WordList,
                        },
                    }
                }),
                sys::ConstraintType::StringList => Some({
                    // SAFETY: By the spec, the union has a value of string_list
                    let data = unsafe { (*self.descriptor).constraint.string_list };
                    DeviceOptionConstraint::ListString(
                        // SAFETY: by spec, this is a null-terminated pointer list
                        // https://sane-project.gitlab.io/standard/api.html#option-value-constraints
                        unsafe { SaneStrListIter::new(data) },
                    )
                }),
                other => Some(DeviceOptionConstraint::Unsupported {
                    value_type,
                    contraint_type: other,
                }),
            }
        })
    }

    pub fn get(&mut self) -> Result<Option<OwnedValue>, Error> {
        self.raw.with_sane(|sane| {
            // SAFETY: reading is synchronized, and the device has not been closed.
            let ty = ValueType::from(unsafe { (*self.descriptor).type_ });

            if ty.is_word_sized() {
                let mut val: sys::Word = 0;
                // SAFETY: Device is not closed, call is synchronized.
                unsafe {
                    sane.sys_get_option_value(
                        self.raw.handle,
                        self.index,
                        (&mut val) as *mut _ as *mut c_void,
                    )
                }?;
                Ok(OwnedValue::from_word(val, ty))
            } else if ty == ValueType::String {
                let mut strbuf = SaneString::with_capacity(self.size());
                // SAFETY: Device is not closed, call is synchronized, strbuf has required capacity.
                unsafe {
                    sane.sys_get_option_value(
                        self.raw.handle,
                        self.index,
                        strbuf.as_mut_ptr() as *mut c_void,
                    )
                }?;
                Ok(Some(OwnedValue::String(strbuf)))
            } else {
                Ok(None)
            }
        })
    }

    pub fn set(&mut self, value: Value) -> Result<(ControlInfo, OwnedValue), Error> {
        self.raw.with_sane(|sane| {
            // SAFETY: Device is not closed, read is synchronized.
            let ty = ValueType::from(unsafe { (*self.descriptor).type_ });
            assert_eq!(
                value.type_of(),
                ty,
                "type of given value does not match type of option"
            );

            if let Some(mut val) = value.to_word() {
                // SAFETY: Device is not closed, call is synchronized.
                let info = unsafe {
                    sane.sys_set_option_value(
                        self.raw.handle,
                        self.index,
                        (&mut val) as *mut _ as *mut c_void,
                    )
                }?;
                Ok((info, OwnedValue::from_word(val, ty).unwrap()))
            } else if let Value::String(s) = value {
                // The documentation doesn't technically require allocating extra space,
                // but this is to be safe.
                let mut strbuf = SaneString::with_capacity(self.size());
                strbuf.set_contents(s);
                // SAFETY: Device is not closed, call is synchronized.
                let info = unsafe {
                    sane.sys_set_option_value(
                        self.raw.handle,
                        self.index,
                        strbuf.as_mut_ptr() as *mut c_void,
                    )
                }?;
                Ok((info, OwnedValue::String(strbuf)))
            } else {
                unreachable!()
            }
        })
    }

    pub fn set_auto(&self) -> Result<(), Error> {
        self.raw
            // SAFETY: Device is not closed, call is synchronized.
            .with_sane(|sane| unsafe { sane.sys_set_option_auto(self.raw.handle, self.index) })
    }
}

impl<S: WithSane> fmt::Debug for DeviceOption<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(stringify!(DeviceOptionDescriptor))
            .field("name", &self.name())
            .field("title", &self.title())
            .field("description", &self.description())
            .field("type", &self.type_())
            .field("unit", &self.unit())
            .field("size", &self.size())
            .field("capabilities", &self.capabilities())
            .field("constraint", &self.constraint())
            .finish()
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(transparent)]
    pub struct DeviceOptionCapabilities: u32 {
        const SOFT_SELECT = sys::CAP_SOFT_SELECT;
        const HARD_SELECT = sys::CAP_HARD_SELECT;
        const CAP_SOFT_DETECT = sys::CAP_SOFT_DETECT;
        const CAP_EMULATED = sys::CAP_EMULATED;
        const CAP_AUTOMATIC = sys::CAP_AUTOMATIC;
        const CAP_INACTIVE = sys::CAP_INACTIVE;
        const CAP_ADVANCED = sys::CAP_ADVANCED;
    }
}

#[derive(Debug)]
pub enum DeviceOptionConstraint<'a> {
    RangeInt {
        min: i32,
        max: i32,
        quant: i32,
    },
    RangeFixed {
        min: Fixed,
        max: Fixed,
        quant: Fixed,
    },
    ListInt(&'a [i32]),
    ListFixed(&'a [Fixed]),
    ListString(SaneStrListIter<'a>),
    Unsupported {
        value_type: sys::ValueType,
        contraint_type: sys::ConstraintType,
    },
}

impl<S: WithSane> DeviceHandle<S> {
    pub fn option(&mut self, index: u32) -> Option<DeviceOption<S>> {
        self.inner.get_option(index)
    }

    pub fn option_count(&mut self) -> usize {
        let mut opt = self.option(0).expect("missing 0th option for count");
        debug_assert_eq!(opt.type_(), ValueType::Int);
        let Ok(Some(OwnedValue::Int(count))) = opt.get() else {
            panic!("cannot get option count");
        };
        count.try_into().unwrap()
    }
}
