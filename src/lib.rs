//! This crate provides a `Vec` wrapper (`Vec1`) which guarantees to have at least 1 element.
//!
//! This can be useful if you have a API which accepts one ore more ofe a kind.
//! Instead of accepting a `Vec` and returning an error if it's empty a `Vec1`
//! can be used assuring there is at least 1 element and through this reducing
//! the number of possible error causes.
//!
//! The crate provides an optional `serde` feature, which provides
//! implementations of `serde::Serialize`/`serde::Deserialize`.
//!
//! # Example
//!
//! ```
//! #[macro_use]
//! extern crate vec1;
//!
//! use vec1::Vec1;
//!
//! fn main() {
//!     // vec1![] makes sure at compiler time
//!     // there is at least one element
//!     //let names = vec1! [ ];
//!     let names = vec1! [ "Liz" ];
//!     greet(names);
//! }
//!
//! fn greet(names: Vec1<&str>) {
//!     // methods like first/last which return a Option on Vec do
//!     // directly return the value, we know it's possible
//!     let first = names.first();
//!     println!("hallo {}", first);
//!     for name in names.iter().skip(1) {
//!         println!("  who is also know as {}", name)
//!     }
//! }
//!
//! ```

use std::{
    borrow::{Borrow, BorrowMut},
    collections::BinaryHeap,
    collections::VecDeque,
    convert::TryFrom,
    error::Error as StdError,
    ffi::CString,
    fmt::{self, Debug},
    iter::{DoubleEndedIterator, ExactSizeIterator, Extend, IntoIterator, Peekable},
    ops::{Bound, Deref, DerefMut, Index, IndexMut, RangeBounds},
    rc::Rc,
    result::Result as StdResult,
    slice,
    sync::Arc,
    vec,
};

/// A macro similar to `vec!` to create a `Vec1`.
///
/// If it is called with less then 1 element a
/// compiler error is triggered (using `compile_error`
/// to make sure you know what went wrong).
#[macro_export]
macro_rules! vec1 {
    () => (
        compile_error!("Vec1 needs at least 1 element")
    );
    ($first:expr $(, $item:expr)* , ) => (
        $crate::vec1!($first $(, $item)*)
    );
    ($first:expr $(, $item:expr)* ) => ({
        #[allow(unused_mut)]
        let mut tmp = $crate::Vec1::new($first);
        $(tmp.push($item);)*
        tmp
    });
}

/// Error returned by operations which would cause `Vec1` to have a length of 0.
#[derive(Debug, Hash, Eq, PartialEq, Copy, Clone)]
pub struct Size0Error;

impl fmt::Display for Size0Error {
    fn fmt(&self, fter: &mut fmt::Formatter) -> fmt::Result {
        #[allow(deprecated)]
        write!(fter, "Cannot produce a Vec1 with a length of zero.")
    }
}
impl StdError for Size0Error {}

type Vec1Result<T> = StdResult<T, Size0Error>;

/// `std::vec::Vec` wrapper which guarantees to have at least 1 element.
///
/// `Vec1<T>` dereferences to `&[T]` and `&mut [T]` as functionality
/// exposed through this can not change the length.
///
/// Methods of `Vec` which can be called without reducing the length
/// (e.g. `capacity()`, `reserve()`) are exposed through wrappers
/// with the same function signature.
///
/// Methods of `Vec` which could reduce the length to 0
/// are implemented with a `try_` prefix returning a `Result`.
/// (e.g. `try_pop(&self)`, `try_truncate()`, etc.).
///
/// Methods with returned `Option<T>` with `None` if the length was 0
/// (and do not reduce the length) now return T. (e.g. `first`,
/// `last`, `first_mut`, etc.).
///
/// All stable traits and methods implemented on `Vec<T>` _should_ also
/// be implemented on `Vec1<T>` (except if they make no sense to implement
/// due to the len 1 guarantee). Note that some small things are still missing
/// e.g. `Vec1` does not implement drain currently as drains generic argument
/// is `R: RangeArgument<usize>` and `RangeArgument` is not stable.
#[derive(Debug, Clone, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Vec1<T>(Vec<T>);

impl<T> IntoIterator for Vec1<T> {
    type Item = T;
    type IntoIter = vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<T> Vec1<T> {
    /// Creates a new `Vec1` instance containing a single element.
    ///
    /// This is roughly `Vec1(vec![first])`.
    pub fn new(first: T) -> Self {
        Vec1(vec![first])
    }

    /// Tries to create a `Vec1<T>` from a `Vec<T>`.
    ///
    /// The fact that the input is returned _as error_ if it's empty,
    /// means that it doesn't work well with the `?` operator. It naming
    /// is also semantic sub-optimal as it's not a "from" but "try from"
    /// conversion. Which is why this method is now deprecated. Instead
    /// use `try_from_vec` and once `TryFrom` is stable it will be possible
    /// to use `try_from`, too.
    ///
    /// # Errors
    ///
    /// If the input is empty the input is returned _as error_.
    #[deprecated(
        since = "1.2.0",
        note = "does not work with `?` use Vec1::try_from_vec() instead"
    )]
    pub fn from_vec(vec: Vec<T>) -> StdResult<Self, Vec<T>> {
        if vec.is_empty() {
            Err(vec)
        } else {
            Ok(Vec1(vec))
        }
    }

    /// Tries to create a `Vec1<T>` from a normal `Vec<T>`.
    ///
    /// # Errors
    ///
    /// This will fail if the input `Vec<T>` is empty.
    /// The returned error is a `Size0Error` instance, as
    /// such this means the _input vector will be dropped if
    /// it's empty_. But this is normally fine as it only
    /// happens if the `Vec<T>` is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// # use vec1::Vec1;
    /// let vec1 = Vec1::try_from_vec(vec![1u8, 2, 3])
    ///     .unwrap();
    /// assert_eq!(vec1, vec![1u8, 2, 3]);
    /// ```
    ///
    /// If you need to return a `Option<Vec1<T>>` you
    /// can use `.ok()` on the returned result:
    ///
    /// ```
    /// # use vec1::Vec1;
    /// fn foobar(input: Vec<u8>) -> Option<Vec1<u8>> {
    ///     let mut res = Vec1::try_from_vec(input).ok()?;
    ///     for x in res.iter_mut() {
    ///         *x *= 2;
    ///     }
    ///     Some(res)
    /// }
    /// ```
    pub fn try_from_vec(vec: Vec<T>) -> Vec1Result<Self> {
        if vec.is_empty() {
            Err(Size0Error)
        } else {
            Ok(Vec1(vec))
        }
    }

    /// Creates a new `Vec1` with a given capacity and a given "first" element.
    pub fn with_capacity(first: T, capacity: usize) -> Self {
        let mut vec = Vec::with_capacity(capacity);
        vec.push(first);
        Vec1(vec)
    }

    /// Turns this `Vec1` into a `Vec`.
    pub fn into_vec(self) -> Vec<T> {
        self.0
    }

    /// Create a new `Vec1` by consuming `self` and mapping each element.
    ///
    /// This is useful as it keeps the knowledge that the length is >= 1,
    /// even through the old `Vec1` is consumed and turned into an iterator.
    ///
    /// # Example
    ///
    /// ```
    /// # #[macro_use]
    /// # extern crate vec1;
    /// # use vec1::Vec1;
    /// # fn main() {
    /// let data = vec1![1u8,2,3];
    ///
    /// let data = data.mapped(|x|x*2);
    /// assert_eq!(data, vec![2,4,6]);
    ///
    /// // without mapped
    /// let data = Vec1::try_from_vec(data.into_iter().map(|x|x*2).collect::<Vec<_>>()).unwrap();
    /// assert_eq!(data, vec![4,8,12]);
    /// # }
    /// ```
    pub fn mapped<F, N>(self, map_fn: F) -> Vec1<N>
    where
        F: FnMut(T) -> N,
    {
        Vec1(self.into_iter().map(map_fn).collect::<Vec<_>>())
    }

    /// Create a new `Vec1` by mapping references to the elements of `self`.
    ///
    /// The benefit to this compared to `Iterator::map` is that it's known
    /// that the length will still be at least 1 when creating the new `Vec1`.
    pub fn mapped_ref<F, N>(&self, map_fn: F) -> Vec1<N>
    where
        F: FnMut(&T) -> N,
    {
        Vec1(self.iter().map(map_fn).collect::<Vec<_>>())
    }

    /// Create a new `Vec1` by mapping mutable references to the elements of `self`.
    ///
    /// The benefit to this compared to `Iterator::map` is that it's known
    /// that the length will still be at least 1 when creating the new `Vec1`.
    pub fn mapped_mut<F, N>(&mut self, map_fn: F) -> Vec1<N>
    where
        F: FnMut(&mut T) -> N,
    {
        Vec1(self.iter_mut().map(map_fn).collect::<Vec<_>>())
    }

    /// Create a new `Vec1` by consuming `self` and mapping each element
    /// to a `Result`.
    ///
    /// This is useful as it keeps the knowledge that the length is >= 1,
    /// even through the old `Vec1` is consumed and turned into an iterator.
    ///
    /// As this method consumes self, returning an error means that this
    /// vec is dropped. I.e. this method behaves roughly like using a
    /// chain of `into_iter()`, `map`, `collect::<Result<Vec<N>,E>>` and
    /// then converting the `Vec` back to a `Vec1`.
    ///
    ///
    /// # Errors
    ///
    /// Once any call to `map_fn` returns a error that error is directly
    /// returned by this method.
    ///
    /// # Example
    ///
    /// ```
    /// # #[macro_use]
    /// # extern crate vec1;
    /// # use vec1::Vec1;
    /// # fn main() {
    /// let data = vec1![1,2,3];
    ///
    /// let data: Result<Vec1<u8>, &'static str> = data.try_mapped(|x| Err("failed"));
    /// assert_eq!(data, Err("failed"));
    /// # }
    /// ```
    pub fn try_mapped<F, N, E>(self, map_fn: F) -> Result<Vec1<N>, E>
    where
        F: FnMut(T) -> Result<N, E>,
    {
        let mut map_fn = map_fn;
        // ::collect<Result<Vec<_>>>() is uses the iterators size hint's lower bound
        // for with_capacity, which is 0 as it might fail at the first element
        let mut out = Vec::with_capacity(self.len());
        for element in self {
            out.push(map_fn(element)?);
        }
        Ok(Vec1(out))
    }

    /// Create a new `Vec1` by mapping references to the elements of `self`
    /// to `Result`s.
    ///
    /// The benefit to this compared to `Iterator::map` is that it's known
    /// that the length will still be at least 1 when creating the new `Vec1`.
    ///
    /// # Errors
    ///
    /// Once any call to `map_fn` returns a error that error is directly
    /// returned by this method.
    ///
    pub fn try_mapped_ref<F, N, E>(&self, map_fn: F) -> Result<Vec1<N>, E>
    where
        F: FnMut(&T) -> Result<N, E>,
    {
        let mut map_fn = map_fn;
        let mut out = Vec::with_capacity(self.len());
        for element in self.iter() {
            out.push(map_fn(element)?);
        }
        Ok(Vec1(out))
    }

    /// Create a new `Vec1` by mapping mutable references to the elements of
    /// `self` to `Result`s.
    ///
    /// The benefit to this compared to `Iterator::map` is that it's known
    /// that the length will still be at least 1 when creating the new `Vec1`.
    ///
    /// # Errors
    ///
    /// Once any call to `map_fn` returns a error that error is directly
    /// returned by this method.
    ///
    pub fn try_mapped_mut<F, N, E>(&mut self, map_fn: F) -> Result<Vec1<N>, E>
    where
        F: FnMut(&mut T) -> Result<N, E>,
    {
        let mut map_fn = map_fn;
        let mut out = Vec::with_capacity(self.len());
        for element in self.iter_mut() {
            out.push(map_fn(element)?);
        }
        Ok(Vec1(out))
    }

    /// Returns a reference to the last element.
    ///
    /// As `Vec1` always contains at least one element there is always a last element.
    pub fn last(&self) -> &T {
        //UNWRAP_SAFE: len is at least 1
        self.0.last().unwrap()
    }

    /// Returns a mutable reference to the last element.
    ///
    /// As `Vec1` always contains at least one element there is always a last element.
    pub fn last_mut(&mut self) -> &mut T {
        //UNWRAP_SAFE: len is at least 1
        self.0.last_mut().unwrap()
    }

    /// Returns a reference to the first element.
    ///
    /// As `Vec1` always contains at least one element there is always a first element.
    pub fn first(&self) -> &T {
        //UNWRAP_SAFE: len is at least 1
        self.0.first().unwrap()
    }

    /// Returns a mutable reference to the first element.
    ///
    /// As `Vec1` always contains at least one element there is always a first element.
    pub fn first_mut(&mut self) -> &mut T {
        //UNWRAP_SAFE: len is at least 1
        self.0.first_mut().unwrap()
    }

    /// Truncates the vec1 to given length.
    ///
    /// # Errors
    ///
    /// If len is 0 an error is returned as the
    /// length >= 1 constraint must be uphold.
    ///
    pub fn try_truncate(&mut self, len: usize) -> Vec1Result<()> {
        if len > 0 {
            self.0.truncate(len);
            Ok(())
        } else {
            Err(Size0Error)
        }
    }

    /// Calls `swap_remove` on the inner vec if length >= 2.
    ///
    /// # Errors
    ///
    /// If len is 1 an error is returned as the
    /// length >= 1 constraint must be uphold.
    pub fn try_swap_remove(&mut self, index: usize) -> Vec1Result<T> {
        if self.len() > 1 {
            Ok(self.0.swap_remove(index))
        } else {
            Err(Size0Error)
        }
    }

    /// Calls `remove` on the inner vec if length >= 2.
    ///
    /// # Errors
    ///
    /// If len is 1 an error is returned as the
    /// length >= 1 constraint must be uphold.
    pub fn try_remove(&mut self, index: usize) -> Vec1Result<T> {
        if self.len() > 1 {
            Ok(self.0.remove(index))
        } else {
            Err(Size0Error)
        }
    }

    /// Calls `split_off` on the inner vec if both resulting parts have length >= 1.
    ///
    /// # Errors
    ///
    /// If after the split any part would be empty an error is returned as the
    /// length >= 1 constraint must be uphold.
    pub fn try_split_off(&mut self, at: usize) -> Vec1Result<Vec1<T>> {
        if at == 0 || at >= self.len() {
            Err(Size0Error)
        } else {
            let out = self.0.split_off(at);
            Ok(Vec1(out))
        }
    }

    /// Calls `dedup_by_key` on the inner vec.
    ///
    /// While this can remove elements it will
    /// never produce a empty vector from an non
    /// empty vector.
    pub fn dedup_by_key<F, K>(&mut self, key: F)
    where
        F: FnMut(&mut T) -> K,
        K: PartialEq<K>,
    {
        self.0.dedup_by_key(key)
    }

    /// Calls `dedup_by_key` on the inner vec.
    ///
    /// While this can remove elements it will
    /// never produce a empty vector from an non
    /// empty vector.
    pub fn dedup_by<F>(&mut self, same_bucket: F)
    where
        F: FnMut(&mut T, &mut T) -> bool,
    {
        self.0.dedup_by(same_bucket)
    }

    /// Tries to remove the last element from the `Vec1`.
    ///
    /// Returns an error if the length is currently 1 (so the `try_pop` would reduce
    /// the length to 0).
    ///
    /// # Errors
    ///
    /// If len is 1 an error is returned as the
    /// length >= 1 constraint must be uphold.
    pub fn try_pop(&mut self) -> Vec1Result<T> {
        if self.len() > 1 {
            //UNWRAP_SAFE: pop on len > 1 can not be none
            Ok(self.0.pop().unwrap())
        } else {
            Err(Size0Error)
        }
    }

    /// Return a reference to the underlying `Vec`.
    pub fn as_vec(&self) -> &Vec<T> {
        &self.0
    }

    /// Calls `splice` on the underlying vec if it will not produce an empty vec.
    ///
    /// # Errors
    ///
    /// If range covers the whole vec and the replacement iterator doesn't yield
    /// any value an error is returned.
    ///
    /// This means that if an error is returned `next` might still have been called
    /// once on the `replace_with` iterator.
    pub fn splice<R, I>(
        &mut self,
        range: R,
        replace_with: I,
    ) -> Vec1Result<Splice<<I as IntoIterator>::IntoIter>>
    where
        I: IntoIterator<Item = T>,
        R: RangeBounds<usize>,
    {
        let mut replace_with = replace_with.into_iter().peekable();
        let range_covers_all = range_covers_vec1(&range, self.len());

        if range_covers_all && replace_with.peek().is_none() {
            Err(Size0Error)
        } else {
            let vec_splice = self.0.splice(range, replace_with);
            Ok(Splice { vec_splice })
        }
    }

    /// Splits off the first element of this vector and returns it together with the rest of the
    /// vector.
    ///
    /// # Examples
    ///
    /// ```
    /// # use vec1::vec1;
    /// assert_eq!((0, vec![]), vec1![0].split_off_first());
    /// assert_eq!((0, vec![1, 2, 3]), vec1![0, 1, 2, 3].split_off_first());
    /// ```
    pub fn split_off_first(self) -> (T, Vec<T>) {
        let mut vec = self.0;
        let first = vec.remove(0);
        (first, vec)
    }

    /// Splits off the last element of this vector and returns it together with the rest of the
    /// vector.
    ///
    /// # Examples
    ///
    /// ```
    /// # use vec1::vec1;
    /// assert_eq!((vec![], 0), vec1![0].split_off_last());
    /// assert_eq!((vec![0, 1, 2], 3), vec1![0, 1, 2, 3].split_off_last());
    /// ```
    pub fn split_off_last(self) -> (Vec<T>, T) {
        let mut vec = self.0;
        let last = vec.remove(vec.len() - 1);
        (vec, last)
    }

}

impl Vec1<u8> {
    /// Works like `&[u8].to_ascii_uppercase()` but returns a `Vec1<T>` instead of a `Vec<T>`
    pub fn to_ascii_uppercase(&self) -> Vec1<u8> {
        Vec1(self.0.to_ascii_uppercase())
    }

    /// Works like `&[u8].to_ascii_lowercase()` but returns a `Vec1<T>` instead of a `Vec<T>`
    pub fn to_ascii_lowercase(&self) -> Vec1<u8> {
        Vec1(self.0.to_ascii_lowercase())
    }
}

fn range_covers_vec1(range: &impl RangeBounds<usize>, vec1_len: usize) -> bool {
    // As this is only used for vec1 we don't need the if vec_len == 0.
    // if vec_len == 0 { return true; }
    range_covers_vec_start(range) && range_covers_vec_end(range, vec1_len)
}

fn range_covers_vec_start(range: &impl RangeBounds<usize>) -> bool {
    match range.start_bound() {
        Bound::Included(idx) => *idx == 0,
        // there is no idx before 0, so if you start from a excluded index
        // you can not cover 0
        Bound::Excluded(_idx) => false,
        Bound::Unbounded => true,
    }
}

fn range_covers_vec_end(range: &impl RangeBounds<usize>, len: usize) -> bool {
    match range.end_bound() {
        Bound::Included(idx) => {
            // covers all if it goes up to the last idx which is len-1
            *idx >= len - 1
        }
        Bound::Excluded(idx) => {
            // len = max_idx + 1, so if excl_end = len it's > max_idx, so >= is correct
            *idx >= len
        }
        Bound::Unbounded => true,
    }
}

pub struct Splice<'a, I: Iterator + 'a> {
    vec_splice: vec::Splice<'a, Peekable<I>>,
}

impl<'a, I> Debug for Splice<'a, I>
where
    I: Iterator + 'a,
    vec::Splice<'a, Peekable<I>>: Debug,
{
    fn fmt(&self, fter: &mut fmt::Formatter) -> fmt::Result {
        fter.debug_tuple("Splice").field(&self.vec_splice).finish()
    }
}

impl<'a, I> Iterator for Splice<'a, I>
where
    I: Iterator,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.vec_splice.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.vec_splice.size_hint()
    }
}

impl<'a, I> ExactSizeIterator for Splice<'a, I> where I: Iterator {}

impl<'a, I> DoubleEndedIterator for Splice<'a, I>
where
    I: Iterator,
{
    fn next_back(&mut self) -> Option<Self::Item> {
        self.vec_splice.next_back()
    }
}

macro_rules! impl_wrapper {
    (pub $T:ident>
        $(fn $name:ident(&$($m:ident)* $(, $param:ident: $tp:ty)*) -> $rt:ty);*) => (
            impl<$T> Vec1<$T> {$(
                #[inline]
                pub fn $name(self: impl_wrapper!{__PRIV_SELF &$($m)*} $(, $param: $tp)*) -> $rt {
                    (self.0).$name($($param),*)
                }
            )*}
    );
    (__PRIV_SELF &mut self) => (&mut Self);
    (__PRIV_SELF &self) => (&Self);
}

// methods in Vec not in &[] which can be directly exposed
impl_wrapper! {
    pub T>
        fn reserve(&mut self, additional: usize) -> ();
        fn reserve_exact(&mut self, additional: usize) -> ();
        fn shrink_to_fit(&mut self) -> ();
        fn as_mut_slice(&mut self) -> &mut [T];
        fn push(&mut self, value: T) -> ();
        fn append(&mut self, other: &mut Vec<T>) -> ();
        fn insert(&mut self, idx: usize, val: T) -> ();
        fn len(&self) -> usize;
        fn capacity(&self) -> usize;
        fn as_slice(&self) -> &[T]
}

impl<T> Vec1<T>
where
    T: Clone,
{
    /// Calls `resize` on the underlying `Vec` if `new_len` >= 1.
    ///
    /// # Errors
    ///
    /// If the `new_len` is 0 an error is returned as
    /// the length >= 1 constraint must be uphold.
    pub fn try_resize(&mut self, new_len: usize, value: T) -> Vec1Result<()> {
        if new_len >= 1 {
            self.0.resize(new_len, value);
            Ok(())
        } else {
            Err(Size0Error)
        }
    }

    pub fn extend_from_slice(&mut self, other: &[T]) {
        self.0.extend_from_slice(other)
    }
}

impl<T> Vec1<T>
where
    T: PartialEq<T>,
{
    pub fn dedub(&mut self) {
        self.0.dedup()
    }
}

impl<T> Vec1<T>
where
    T: PartialEq<T>,
{
    pub fn dedup(&mut self) {
        self.0.dedup()
    }
}

impl<T> Default for Vec1<T>
where
    T: Default,
{
    fn default() -> Self {
        Vec1::new(Default::default())
    }
}

impl<T> Deref for Vec1<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Vec1<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Into<Vec<T>> for Vec1<T> {
    fn into(self) -> Vec<T> {
        self.0
    }
}

impl<A, B> PartialEq<Vec1<B>> for Vec1<A>
where
    A: PartialEq<B>,
{
    fn eq(&self, other: &Vec1<B>) -> bool {
        self.0.eq(&other.0)
    }
}

impl<A, B> PartialEq<B> for Vec1<A>
where
    Vec<A>: PartialEq<B>,
{
    fn eq(&self, other: &B) -> bool {
        self.0.eq(other)
    }
}

impl<T, O, R> Index<R> for Vec1<T>
where
    Vec<T>: Index<R, Output = O>,
    O: ?Sized,
{
    type Output = O;

    fn index(&self, index: R) -> &O {
        self.0.index(index)
    }
}

impl<T, O, R> IndexMut<R> for Vec1<T>
where
    Vec<T>: IndexMut<R, Output = O>,
    O: ?Sized,
{
    fn index_mut(&mut self, index: R) -> &mut Self::Output {
        self.0.index_mut(index)
    }
}

impl<T> Borrow<[T]> for Vec1<T> {
    fn borrow(&self) -> &[T] {
        self
    }
}

impl<T> BorrowMut<[T]> for Vec1<T> {
    fn borrow_mut(&mut self) -> &mut [T] {
        self
    }
}

impl<T> Borrow<Vec<T>> for Vec1<T> {
    fn borrow(&self) -> &Vec<T> {
        &self.0
    }
}

impl<'a, T> Extend<&'a T> for Vec1<T>
where
    T: 'a + Copy,
{
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = &'a T>,
    {
        self.0.extend(iter)
    }
}

impl<T> Extend<T> for Vec1<T> {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = T>,
    {
        self.0.extend(iter)
    }
}

impl<T> AsRef<[T]> for Vec1<T> {
    fn as_ref(&self) -> &[T] {
        self
    }
}

impl<T> AsMut<[T]> for Vec1<T> {
    fn as_mut(&mut self) -> &mut [T] {
        self
    }
}

impl<T> AsRef<Vec<T>> for Vec1<T> {
    fn as_ref(&self) -> &Vec<T> {
        &self.0
    }
}
impl<T> AsRef<Vec1<T>> for Vec1<T> {
    fn as_ref(&self) -> &Vec1<T> {
        self
    }
}

impl<T> AsMut<Vec1<T>> for Vec1<T> {
    fn as_mut(&mut self) -> &mut Vec1<T> {
        self
    }
}

impl<'a, T> IntoIterator for &'a Vec1<T> {
    type Item = &'a T;
    type IntoIter = slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
impl<'a, T> IntoIterator for &'a mut Vec1<T> {
    type Item = &'a mut T;
    type IntoIter = slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}

#[cfg(feature = "serde")]
impl<'de, T> ::serde::Deserialize<'de> for Vec1<T>
where
    T: ::serde::Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        use ::serde::de::Error;

        let v = Vec::deserialize(deserializer)?;
        let v1 = Vec1::try_from_vec(v).map_err(|e| D::Error::custom(e))?;

        Ok(v1)
    }
}

impl<T> Into<Rc<[T]>> for Vec1<T> {
    fn into(self) -> Rc<[T]> {
        self.0.into()
    }
}

impl<T> Into<Arc<[T]>> for Vec1<T> {
    fn into(self) -> Arc<[T]> {
        self.0.into()
    }
}

impl<T> std::convert::TryFrom<Vec<T>> for Vec1<T> {
    type Error = Size0Error;

    fn try_from(vec: Vec<T>) -> StdResult<Self, Self::Error> {
        Vec1::try_from_vec(vec)
    }
}

macro_rules! wrapper_from_to_try_from {
    (impl Into + impl[$($tv:tt)*] TryFrom<$tf:ty> for Vec1<$et:ty> $($tail:tt)*) => (

        wrapper_from_to_try_from!(impl[$($tv),*] TryFrom<$tf> for Vec1<$et> $($tail)*);

        impl<$($tv)*> Into<$tf> for Vec1<$et> $($tail)* {
            fn into(self) -> $tf {
                self.0.into()
            }
        }
    );
    (impl[$($tv:tt)*] TryFrom<$tf:ty> for Vec1<$et:ty> $($tail:tt)*) => (
        impl<$($tv)*> TryFrom<$tf> for Vec1<$et> $($tail)* {
            type Error = Size0Error;

            fn try_from(inp: $tf) -> StdResult<Self, Self::Error> {
                if inp.is_empty() {
                    Err(Size0Error)
                } else {
                    Ok(Vec1(inp.into()))
                }
            }
        }
    );
}

wrapper_from_to_try_from!(impl Into + impl[T] TryFrom<Box<[T]>> for Vec1<T>);
wrapper_from_to_try_from!(impl[T] TryFrom<BinaryHeap<T>> for Vec1<T>);
wrapper_from_to_try_from!(impl[] TryFrom<String> for Vec1<u8>);
wrapper_from_to_try_from!(impl['a] TryFrom<&'a str> for Vec1<u8>);
wrapper_from_to_try_from!(impl['a, T] TryFrom<&'a [T]> for Vec1<T> where T: Clone);
wrapper_from_to_try_from!(impl['a, T] TryFrom<&'a mut [T]> for Vec1<T> where T: Clone);
wrapper_from_to_try_from!(impl Into + impl[T] TryFrom<VecDeque<T>> for Vec1<T>);

/// **Warning: This impl is unstable and requires nightly,
///   it's not covert by semver stability guarantees.**
impl TryFrom<CString> for Vec1<u8> {
    type Error = Size0Error;

    /// Like `Vec`'s `From<CString>` this will treat the `'\0'` as not part of the string.
    fn try_from(string: CString) -> StdResult<Self, Self::Error> {
        if string.as_bytes().is_empty() {
            Err(Size0Error)
        } else {
            Ok(Vec1(string.into()))
        }
    }
}

#[cfg(test)]
mod test {

    #[macro_export]
    macro_rules! assert_ok {
        ($val:expr) => {{
            match $val {
                Ok(res) => res,
                Err(err) => panic!("expected Ok(..) got Err({:?})", err),
            }
        }};
        ($val:expr, $ctx:expr) => {{
            match $val {
                Ok(res) => res,
                Err(err) => panic!("expected Ok(..) got Err({:?}) [ctx: {:?}]", err, $ctx),
            }
        }};
    }

    macro_rules! assert_err {
        ($val:expr) => {{
            match $val {
                Ok(val) => panic!("expected Err(..) got Ok({:?})", val),
                Err(err) => err,
            }
        }};
        ($val:expr, $ctx:expr) => {{
            match $val {
                Ok(val) => panic!("expected Err(..) got Ok({:?}) [ctx: {:?}]", val, $ctx),
                Err(err) => err,
            }
        }};
    }

    mod Size0Error {
        #![allow(non_snake_case)]
        use super::super::*;

        #[test]
        fn implements_std_error() {
            fn comp_check<T: StdError>() {}
            comp_check::<Size0Error>();
        }
    }

    #[test]
    fn range_covers_vec() {
        use super::range_covers_vec1;

        let len = 3;
        // common slicesa
        assert!(range_covers_vec1(&(..), len));
        assert!(range_covers_vec1(&(..3), len));
        assert!(!range_covers_vec1(&(..2), len));
        assert!(!range_covers_vec1(&(1..3), len));
        assert!(range_covers_vec1(&(0..3), len));
        assert!(range_covers_vec1(&(0..), len));
        assert!(!range_covers_vec1(&(1..), len));
        assert!(!range_covers_vec1(&(len..), len));

        // unusual slices
        assert!(!range_covers_vec1(&(..0), len));
        assert!(!range_covers_vec1(&(2..1), len));
    }

    mod Vec1 {
        #![allow(non_snake_case)]
        use super::super::*;

        #[test]
        fn deref_slice() {
            let vec = Vec1::new(1u8);
            let _: &[u8] = &*vec;
        }

        #[test]
        fn deref_slice_mut() {
            let mut vec = Vec1::new(1u8);
            let _: &mut [u8] = &mut *vec;
        }

        #[test]
        fn provided_all_ro_functions() {
            let vec = Vec1::new(1u8);
            assert_eq!(vec.len(), 1);
            assert!(vec.capacity() > 0);
            assert_eq!(vec.as_slice(), &*vec);
            // there is obviously no reason we should provide this,
            // as it can't be empty at all, that's the point behind Vec1
            //assert_eq!(vec.is_empty(), true)
        }

        #[test]
        fn provides_some_safe_mut_functions() {
            let mut vec = Vec1::new(1u8);
            vec.reserve(12);
            assert!(vec.capacity() >= 13);
            vec.reserve_exact(31);
            assert!(vec.capacity() >= 31);
            vec.shrink_to_fit();
            let _: &mut [u8] = vec.as_mut_slice();
            vec.insert(1, 31u8);
            vec.insert(1, 2u8);
            assert_eq!(&*vec, &[1, 2, 31]);
            vec.dedup_by_key(|k| *k / 3);
            assert_eq!(&*vec, &[1, 31]);
            vec.push(31);
            assert_eq!(&*vec, &[1, 31, 31]);
            vec.dedup_by(|l, r| l == r);
            assert_eq!(&*vec, &[1, 31]);
            vec.extend_from_slice(&[31, 2, 3]);
            assert_eq!(&*vec, &[1, 31, 31, 2, 3]);
            vec.dedub();
            assert_eq!(&*vec, &[1, 31, 2, 3]);
            // as the passed in vec is emptied this won't work with a vec1 as parameter
            vec.append(&mut vec![1, 2, 3]);
            assert_eq!(&*vec, &[1, 31, 2, 3, 1, 2, 3])
        }

        #[test]
        fn provides_other_methos_in_failible_form() {
            let mut vec = vec1![1u8, 2, 3, 4];
            assert_ok!(vec.try_truncate(3));
            assert_err!(vec.try_truncate(0));
            assert_eq!(vec, &[1, 2, 3]);

            assert_ok!(vec.try_swap_remove(0));
            assert_eq!(vec, &[3, 2]);
            assert_ok!(vec.try_remove(0));
            assert_eq!(vec, &[2]);
            assert_err!(vec.try_swap_remove(0));
            assert_err!(vec.try_remove(0));
            vec.push(12);

            assert_eq!(vec.try_pop(), Ok(12));
            assert_eq!(vec.try_pop(), Err(Size0Error));
            assert_eq!(vec, &[2]);
        }

        #[test]
        fn try_split_of() {
            let mut vec = vec1![1, 2, 3, 4];
            assert_err!(vec.try_split_off(0));
            let len = vec.len();
            assert_err!(vec.try_split_off(len));
            let nvec = assert_ok!(vec.try_split_off(len - 1));
            assert_eq!(vec, &[1, 2, 3]);
            assert_eq!(nvec, &[4]);
        }

        #[test]
        fn try_resize() {
            let mut vec = Vec1::new(1u8);
            assert_ok!(vec.try_resize(10, 2u8));
            assert_eq!(vec.len(), 10);
            assert_ok!(vec.try_resize(1, 2u8));
            assert_eq!(vec, &[1]);
            assert_err!(vec.try_resize(0, 2u8));
        }

        #[test]
        fn with_capacity() {
            let vec = Vec1::with_capacity(1u8, 16);
            assert!(vec.capacity() >= 16);
        }

        #[test]
        fn impl_index() {
            let vec = vec1![1, 2, 3, 3];
            assert_eq!(&vec[..2], &[1, 2]);
        }
        #[test]
        fn impl_index_mut() {
            let mut vec = vec1![1, 2, 3, 3];
            assert_eq!(&mut vec[..2], &mut [1, 2]);
        }

        #[test]
        fn impl_extend() {
            let mut vec = Vec1::new(1u8);
            vec.extend([2, 3].iter().cloned());
            assert_eq!(vec, &[1, 2, 3]);
        }

        #[test]
        fn impl_extend_ref_copy() {
            let mut vec = Vec1::new(1u8);
            vec.extend([2, 3].iter());
            assert_eq!(vec, &[1, 2, 3]);
        }

        #[test]
        fn impl_borrow_mut_slice() {
            fn chk<E, T: BorrowMut<[E]>>() {};
            chk::<u8, Vec1<u8>>();
        }

        #[test]
        fn impl_borrow_slice() {
            fn chk<E, T: BorrowMut<[E]>>() {};
            chk::<u8, Vec1<u8>>();
        }

        #[test]
        fn impl_as_mut_slice() {
            fn chk<E, T: AsMut<[E]>>() {};
            chk::<u8, Vec1<u8>>();
        }

        #[test]
        fn impl_as_ref() {
            fn chk<E, T: AsRef<[E]>>() {};
            chk::<u8, Vec1<u8>>();
        }
        #[test]
        fn impl_as_mut_slice_self() {
            fn chk<E, T: AsMut<Vec1<E>>>() {};
            chk::<u8, Vec1<u8>>();
        }

        #[test]
        fn impl_as_ref_self() {
            fn chk<E, T: AsRef<Vec1<E>>>() {};
            chk::<u8, Vec1<u8>>();
        }

        #[test]
        fn impl_as_ref_vec() {
            fn chk<E, T: AsRef<Vec<E>>>() {};
            chk::<u8, Vec1<u8>>();
        }

        //into iter self, &, &mut
        #[test]
        fn impl_into_iter() {
            let vec = vec1![1, 2, 3];
            assert_eq!(6, vec.into_iter().sum::<u8>());
        }
        #[test]
        fn impl_into_iter_on_ref() {
            let vec = vec1![1, 2, 3];
            assert_eq!(6, (&vec).into_iter().sum::<u8>());
        }
        #[test]
        fn impl_into_iter_on_ref_mut() {
            let mut vec = vec1![1, 2, 3];
            assert_eq!(
                3,
                (&mut vec).into_iter().fold(0u8, |x, m| {
                    *m = *m + 1;
                    x + 1
                })
            );
            assert_eq!(vec, &[2, 3, 4]);
        }

        #[test]
        fn non_slice_indexing_works() {
            let mut vec = vec1!["a"];
            assert_eq!(&mut vec[0], &mut "a");
        }

        #[test]
        fn splice_with_full_range_and_no_replace_values_fails() {
            let mut vec = Vec1::try_from_vec(vec![1, 2, 3, 4, 5]).unwrap();
            let res = vec.splice(.., vec![]);
            assert!(res.is_err());
        }

        #[test]
        fn splice_with_full_range_but_non_empty_iter_works() {
            let mut vec = Vec1::try_from_vec(vec![1, 2, 3, 4, 5]).unwrap();
            let res: Vec<_> = vec.splice(.., vec![11]).unwrap().collect();
            assert_eq!(res, vec![1, 2, 3, 4, 5]);
            assert_eq!(vec, vec![11]);
        }

        #[test]
        fn splice_with_non_full_range_but_empty_iter_works() {
            let mut vec = Vec1::try_from_vec(vec![1, 2, 3, 4, 5]).unwrap();
            let res: Vec<_> = vec.splice(1.., vec![]).unwrap().collect();

            assert_eq!(res, vec![2, 3, 4, 5]);
            assert_eq!(vec, vec![1]);
        }

        #[test]
        fn deriving_default_works() {
            #[derive(Default)]
            struct Example {
                field: Vec1<u8>,
            }

            let example = Example::default();

            assert_eq!(example.field, vec1![0]);
        }

        #[cfg(feature = "serde")]
        mod serde {
            use super::super::super::*;

            #[test]
            fn empty() {
                let result: Result<Vec1<u8>, _> = serde_json::from_str("[]");
                assert!(result.is_err());
            }

            #[test]
            fn one_element() {
                let vec: Vec1<u8> = serde_json::from_str("[1]").unwrap();
                assert_eq!(vec, vec1![1]);
                let json = serde_json::to_string(&vec).unwrap();
                assert_eq!(json, "[1]");
            }

            #[test]
            fn multiple_elements() {
                let vec: Vec1<u8> = serde_json::from_str("[1, 2, 3]").unwrap();
                assert_eq!(vec, vec1![1, 2, 3]);
                let json = serde_json::to_string(&vec).unwrap();
                assert_eq!(json, "[1,2,3]");
            }
        }

        #[test]
        fn has_a_try_from_impl() {
            use std::convert::TryFrom;

            let vec = Vec1::<u8>::try_from(vec![]);
            assert_eq!(vec, Err(Size0Error));

            let vec = Vec1::try_from(vec![1u8, 12]).unwrap();
            assert_eq!(vec, vec![1u8, 12]);
        }

        #[test]
        fn has_a_try_from_boxed_slice() {
            use std::convert::TryFrom;
            let bs: Box<[u8]> = vec![1, 2, 3].into();
            let vec = Vec1::<u8>::try_from(bs).unwrap();
            assert_eq!(vec, vec![1u8, 2, 3]);
        }
    }
}
