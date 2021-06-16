//! Relative pointer implementations and options.

#[cfg(feature = "validation")]
mod validation;

use core::{
    convert::TryFrom,
    fmt,
    marker::{PhantomData, PhantomPinned},
    mem::MaybeUninit,
    ptr,
};
use crate::{
    Archived,
    ArchivePointee,
    ArchiveUnsized,
};

/// The offset between the two positions cannot be represented by the offset type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetError {
    /// The offset overflowed the range of `isize`
    IsizeOverflow,
    /// The offset is too far for the offset type of the relative pointer
    ExceedsStorageRange,
}

impl fmt::Display for OffsetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OffsetError::IsizeOverflow => write!(f, "the offset overflowed the range of `isize`"),
            OffsetError::ExceedsStorageRange => write!(f, "the offset is too far for the offset type of the relative pointer"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for OffsetError {}

/// Calculates the offset between two positions as an `isize`.
///
/// This function exists solely to get the distance between two `usizes` as an `isize` with a full
/// range of values.
///
/// # Examples
///
/// ```
/// use rkyv::rel_ptr::{signed_offset, OffsetError};
///
/// assert_eq!(signed_offset(0, 1), Ok(1));
/// assert_eq!(signed_offset(1, 0), Ok(-1));
/// assert_eq!(signed_offset(0, isize::MAX as usize), Ok(isize::MAX));
/// assert_eq!(signed_offset(isize::MAX as usize, 0), Ok(-isize::MAX));
/// assert_eq!(signed_offset(0, isize::MAX as usize + 1), Err(OffsetError::IsizeOverflow));
/// assert_eq!(signed_offset(isize::MAX as usize + 1, 0), Ok(isize::MIN));
/// assert_eq!(signed_offset(0, isize::MAX as usize + 2), Err(OffsetError::IsizeOverflow));
/// assert_eq!(signed_offset(isize::MAX as usize + 2, 0), Err(OffsetError::IsizeOverflow));
/// ```
#[inline]
pub fn signed_offset(from: usize, to: usize) -> Result<isize, OffsetError> {
    let (result, overflow) = to.overflowing_sub(from);
    if (!overflow && result <= (isize::MAX as usize)) || (overflow && result >= (isize::MIN as usize)) {
        Ok(result as isize)
    } else {
        Err(OffsetError::IsizeOverflow)
    }
}

/// A offset that can be used with [`RawRelPtr`].
pub trait Offset: Copy {
    /// Any error that can be produced while creating an offset.
    type Error;

    /// Creates a new offset between a `from` position and a `to` position.
    fn between(from: usize, to: usize) -> Result<Self, Self::Error>;

    /// Gets the offset as an `isize`.
    fn to_isize(self) -> isize;
}

macro_rules! impl_offset {
    ($ty:ty) => {
        impl Offset for Archived<$ty> {
            type Error = OffsetError;

            #[inline]
            fn between(from: usize, to: usize) -> Result<Self, Self::Error> {
                // pointer::add and pointer::offset require that the computed offsets cannot
                // overflow an isize, which is why we're using signed_offset instead of checked_sub
                // for unsized types
                <$ty>::try_from(signed_offset(from, to)?).map_err(|_| OffsetError::ExceedsStorageRange)
            }

            #[inline]
            fn to_isize(self) -> isize {
                // We're guaranteed that our offset will not exceed the the capacity of an `isize`
                self as isize
            }
        }
    };
}

impl_offset!(i8);
impl_offset!(i16);
#[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
impl_offset!(i32);
#[cfg(target_pointer_width = "64")]
impl_offset!(i64);
impl_offset!(u8);
impl_offset!(u16);
#[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
impl_offset!(u32);
#[cfg(target_pointer_width = "64")]
impl_offset!(u64);

/// Errors that can occur while creating raw relative pointers.
#[derive(Debug)]
pub enum RelPtrError {
    /// The given `from` and `to` positions for the relative pointer failed to form a valid offset.
    ///
    /// This is probably because the distance between them could not be represented by the offset
    /// type.
    OffsetError,
}

/// An untyped pointer which resolves relative to its position in memory.
#[derive(Debug)]
#[repr(transparent)]
pub struct RawRelPtr<O> {
    offset: O,
    _phantom: PhantomPinned,
}

impl<O: Offset> RawRelPtr<O> {
    /// Creates a new `RawRelPtr` in-place between the given `from` and `to` positions.
    ///
    /// # Safety
    ///
    /// - `out` must be located at position `from`
    /// - `to` must be a position within the archive
    #[inline]
    pub unsafe fn emplace(from: usize, to: usize, out: &mut MaybeUninit<Self>) -> Result<(), O::Error> {
        let offset = O::between(from, to)?;
        ptr::addr_of_mut!((*out.as_mut_ptr()).offset)
            .write(to_archived!(offset));
        Ok(())
    }

    /// Gets the base pointer for the relative pointer.
    #[inline]
    pub fn base(&self) -> *const u8 {
        (self as *const Self).cast::<u8>()
    }

    /// Gets the mutable base pointer for the relative pointer.
    #[inline]
    pub fn base_mut(&mut self) -> *mut u8 {
        (self as *mut Self).cast::<u8>()
    }

    /// Gets the offset of the relative pointer from its base.
    #[inline]
    pub fn offset(&self) -> isize {
        self.offset.to_isize()
    }

    /// Calculates the memory address being pointed to by this relative pointer.
    #[inline]
    pub fn as_ptr(&self) -> *const () {
        unsafe {
            self.base().offset(self.offset()).cast()
        }
    }

    /// Returns an unsafe mutable pointer to the memory address being pointed to
    /// by this relative pointer.
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut () {
        unsafe {
            self.base_mut().offset(self.offset()).cast()
        }
    }
}

/// A raw relative pointer that uses an archived `i8` as the underlying offset.
pub type RawRelPtrI8 = RawRelPtr<Archived<i8>>;
/// A raw relative pointer that uses an archived `i16` as the underlying offset.
pub type RawRelPtrI16 = RawRelPtr<Archived<i16>>;
/// A raw relative pointer that uses an archived `i32` as the underlying offset.
#[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
pub type RawRelPtrI32 = RawRelPtr<Archived<i32>>;
/// A raw relative pointer that uses an archived `i64` as the underlying offset.
#[cfg(target_pointer_width = "64")]
pub type RawRelPtrI64 = RawRelPtr<Archived<i64>>;

/// A raw relative pointer that uses an archived `u8` as the underlying offset.
pub type RawRelPtrU8 = RawRelPtr<Archived<u8>>;
/// A raw relative pointer that uses an archived `u16` as the underlying offset.
pub type RawRelPtrU16 = RawRelPtr<Archived<u16>>;
/// A raw relative pointer that uses an archived `u32` as the underlying offset.
#[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
pub type RawRelPtrU32 = RawRelPtr<Archived<u32>>;
/// A raw relative pointer that uses an archived `u64` as the underlying offset.
#[cfg(target_pointer_width = "64")]
pub type RawRelPtrU64 = RawRelPtr<Archived<u64>>;

// TOOD: implement for NonZero types

/// A pointer which resolves to relative to its position in memory.
///
/// See [`Archive`](crate::Archive) for an example of creating one.
pub struct RelPtr<T: ArchivePointee + ?Sized, O> {
    raw_ptr: RawRelPtr<O>,
    metadata: T::ArchivedMetadata,
    _phantom: PhantomData<T>,
}

impl<T: ArchivePointee + ?Sized, O: Offset> RelPtr<T, O> {
    /// Creates a relative pointer from one position to another.
    ///
    /// # Safety
    ///
    /// - `from` must be the position of `out` within the archive
    /// - `to` must be the position of some valid `T`
    /// - `value` must be the value being serialized
    /// - `metadata_resolver` must be the result of serializing the metadata of `value`
    #[inline]
    pub unsafe fn resolve_emplace<U: ArchiveUnsized<Archived = T> + ?Sized>(
        from: usize,
        to: usize,
        value: &U,
        metadata_resolver: U::MetadataResolver,
        out: &mut MaybeUninit<Self>,
    ) -> Result<(), O::Error> {
        let (fp, fo) = out_field!(out.raw_ptr);
        RawRelPtr::emplace(from + fp, to, fo)?;
        let (fp, fo) = out_field!(out.metadata);
        value.resolve_metadata(from + fp, metadata_resolver, fo);
        Ok(())
    }

    /// Gets the base pointer for the relative pointer.
    #[inline]
    pub fn base(&self) -> *const u8 {
        self.raw_ptr.base()
    }

    /// Gets the mutable base pointer for the relative pointer.
    #[inline]
    pub fn base_mut(&mut self) -> *mut u8 {
        self.raw_ptr.base_mut()
    }

    /// Gets the offset of the relative pointer from its base.
    #[inline]
    pub fn offset(&self) -> isize {
        self.raw_ptr.offset()
    }

    /// Gets the metadata of the relative pointer.
    #[inline]
    pub fn metadata(&self) -> &T::ArchivedMetadata {
        &self.metadata
    }

    /// Calculates the memory address being pointed to by this relative pointer.
    #[inline]
    pub fn as_ptr(&self) -> *const T {
        ptr_meta::from_raw_parts(self.raw_ptr.as_ptr(), T::pointer_metadata(&self.metadata))
    }

    /// Returns an unsafe mutable pointer to the memory address being pointed to by this relative
    /// pointer.
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        ptr_meta::from_raw_parts_mut(
            self.raw_ptr.as_mut_ptr(),
            T::pointer_metadata(&self.metadata),
        )
    }
}

impl<T: ArchivePointee + ?Sized, O: fmt::Debug> fmt::Debug for RelPtr<T, O>
where
    T::ArchivedMetadata: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelPtr")
            .field("raw_ptr", &self.raw_ptr)
            .field("metadata", &self.metadata)
            .field("_phantom", &self._phantom)
            .finish()
    }
}