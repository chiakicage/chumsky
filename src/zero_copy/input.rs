//! Token input streams and tools converting to and from them..
//!
//! *“What’s up?” “I don’t know,” said Marvin, “I’ve never been there.”*
//!
//! [`Input`] is the primary trait used to feed input data into a chumsky parser. You can create them in a number of
//! ways: from strings, slices, arrays, etc.

use super::*;
use core::cell::Cell;
use hashbrown::HashMap;

/// A trait for types that represents a stream of input tokens. Unlike [`Iterator`], this type
/// supports backtracking and a few other features required by the crate.
// TODO: Remove `Clone` bound
pub trait Input<'a>: 'a {
    /// The type used to keep track of the current location in the stream
    type Offset: Copy + Hash + Ord + Into<usize>;
    /// The type of singular items read from the stream
    type Token;
    /// The type of a span on this input - to provide custom span context see [`WithContext`]
    type Span: Span;

    /// Get the offset representing the start of this stream
    fn start(&self) -> Self::Offset;

    /// Get the next offset from the provided one, and the next token if it exists
    ///
    /// Safety: `offset` must be generated be generated by either `Input::start` or a previous call to this function,
    /// on this input only.
    unsafe fn next(&self, offset: Self::Offset) -> (Self::Offset, Option<Self::Token>);

    /// Create a span from a start and end offset
    fn span(&self, range: Range<Self::Offset>) -> Self::Span;

    #[doc(hidden)]
    fn reborrow(&self) -> Self;
}

/// A trait for types that represent slice-like streams of input tokens.
pub trait SliceInput<'a>: Input<'a> {
    /// The unsized slice type of this input. For [`&str`] it's `str`, and for [`&[T]`] it will be
    /// `[T]`
    type Slice;

    /// Get a slice from a start and end offset
    fn slice(&self, range: Range<Self::Offset>) -> Self::Slice;
    /// Get a slice from a start offset till the end of the input
    fn slice_from(&self, from: RangeFrom<Self::Offset>) -> Self::Slice;
}

// Implemented by inputs that reference a string slice and use byte indices as their offset.
/// A trait for types that represent string-like streams of input tokens
pub trait StrInput<'a, C: Char>:
    Input<'a, Offset = usize, Token = C> + SliceInput<'a, Slice = &'a C::Str>
{
}

/// Implemented by inputs that can have tokens borrowed from them.
pub trait BorrowInput<'a>: Input<'a> {
    /// See [`Input::next`].
    unsafe fn next_ref(&self, offset: Self::Offset) -> (Self::Offset, Option<&'a Self::Token>);
}

impl<'a> Input<'a> for &'a str {
    type Offset = usize;
    type Token = char;
    type Span = SimpleSpan<usize>;

    fn start(&self) -> Self::Offset {
        0
    }

    #[inline]
    unsafe fn next(&self, offset: Self::Offset) -> (Self::Offset, Option<Self::Token>) {
        if offset < self.len() {
            let c = unsafe {
                self.get_unchecked(offset..)
                    .chars()
                    .next()
                    .unwrap_unchecked()
            };
            (offset + c.len_utf8(), Some(c))
        } else {
            (offset, None)
        }
    }

    #[inline]
    fn span(&self, range: Range<Self::Offset>) -> Self::Span {
        range.into()
    }

    fn reborrow(&self) -> Self {
        *self
    }
}

impl<'a> StrInput<'a, char> for &'a str {}

impl<'a> SliceInput<'a> for &'a str {
    type Slice = &'a str;

    #[inline]
    fn slice(&self, range: Range<Self::Offset>) -> Self::Slice {
        &self[range]
    }

    #[inline]
    fn slice_from(&self, from: RangeFrom<Self::Offset>) -> Self::Slice {
        &self[from]
    }
}

impl<'a, T: Clone> Input<'a> for &'a [T] {
    type Offset = usize;
    type Token = T;
    type Span = SimpleSpan<usize>;

    fn start(&self) -> Self::Offset {
        0
    }

    #[inline]
    unsafe fn next(&self, offset: Self::Offset) -> (Self::Offset, Option<Self::Token>) {
        let (offset, tok) = self.next_ref(offset);
        (offset, tok.cloned())
    }

    #[inline]
    fn span(&self, range: Range<Self::Offset>) -> Self::Span {
        range.into()
    }

    fn reborrow(&self) -> Self {
        *self
    }
}

impl<'a> StrInput<'a, u8> for &'a [u8] {}

impl<'a, T: Clone> SliceInput<'a> for &'a [T] {
    type Slice = &'a [T];

    #[inline]
    fn slice(&self, range: Range<Self::Offset>) -> Self::Slice {
        &self[range]
    }

    #[inline]
    fn slice_from(&self, from: RangeFrom<Self::Offset>) -> Self::Slice {
        &self[from]
    }
}

impl<'a, T: Clone> BorrowInput<'a> for &'a [T] {
    unsafe fn next_ref(&self, offset: Self::Offset) -> (Self::Offset, Option<&'a Self::Token>) {
        if let Some(tok) = self.get(offset) {
            (offset + 1, Some(tok))
        } else {
            // We actually don't care if the offset goes beyond the end of the slice, and this seems to be *slightly* faster
            (offset, None)
        }
    }
}

/// An input wrapper contains a user-defined context in its span, in addition to the span of the
/// wrapped input.
#[derive(Copy, Clone)]
pub struct WithContext<Ctx, I>(pub Ctx, pub I);

impl<Ctx, I> WithContext<Ctx, I> {
    /// Create a new [`WithContext`].
    pub fn new(ctx: Ctx, inp: I) -> Self {
        Self(ctx, inp)
    }
}

impl<'a, Ctx: Clone + 'a, I: Input<'a>> Input<'a> for WithContext<Ctx, I> {
    type Offset = I::Offset;
    type Token = I::Token;
    type Span = (Ctx, I::Span);

    fn start(&self) -> Self::Offset {
        self.1.start()
    }

    unsafe fn next(&self, offset: Self::Offset) -> (Self::Offset, Option<Self::Token>) {
        self.1.next(offset)
    }

    fn span(&self, range: Range<Self::Offset>) -> Self::Span {
        (self.0.clone(), self.1.span(range))
    }

    fn reborrow(&self) -> Self {
        WithContext(self.0.clone(), self.1.reborrow())
    }
}

impl<'a, Ctx: Clone + 'a, I: BorrowInput<'a>> BorrowInput<'a> for WithContext<Ctx, I> {
    unsafe fn next_ref(&self, offset: Self::Offset) -> (Self::Offset, Option<&'a Self::Token>) {
        self.1.next_ref(offset)
    }
}

impl<'a, Ctx: Clone + 'a, I: SliceInput<'a>> SliceInput<'a> for WithContext<Ctx, I> {
    type Slice = I::Slice;

    fn slice(&self, range: Range<Self::Offset>) -> Self::Slice {
        <I as SliceInput>::slice(&self.1, range)
    }
    fn slice_from(&self, from: RangeFrom<Self::Offset>) -> Self::Slice {
        <I as SliceInput>::slice_from(&self.1, from)
    }
}

impl<'a, Ctx, C, I> StrInput<'a, C> for WithContext<Ctx, I>
where
    Ctx: Clone + 'a,
    C: Char,
    I: StrInput<'a, C>,
{
}

/// An input that dynamically pulls tokens from an [`Iterator`].
///
/// Internally, the stream will pull tokens in batches so as to avoid invoking the iterator every time a new token is
/// required.
pub struct Stream<I: Iterator>(Cell<(Vec<I::Item>, Option<I>)>);

impl<I: Iterator> Stream<I> {
    /// Box this stream, turning it into a [BoxedStream]. This can be useful in cases where your parser accepts input
    /// from several different sources and it needs to work with all of them.
    pub fn boxed<'a>(self) -> BoxedStream<'a, I::Item>
    where
        I: 'a,
    {
        let (vec, iter) = self.0.into_inner();
        Stream(Cell::new((
            vec,
            Some(Box::new(iter.expect("no iterator?!"))),
        )))
    }
}

/// A stream containing a boxed iterator. See [`Stream::boxed`].
pub type BoxedStream<'a, T> = Stream<Box<dyn Iterator<Item = T> + 'a>>;

impl<'a, I: Iterator> Input<'a> for &'a Stream<I>
where
    I::Item: Clone,
{
    type Offset = usize;
    type Token = I::Item;
    type Span = SimpleSpan<usize>;

    fn start(&self) -> Self::Offset {
        0
    }

    unsafe fn next(&self, offset: Self::Offset) -> (Self::Offset, Option<Self::Token>) {
        let mut other = Cell::new((Vec::new(), None));
        self.0.swap(&other);

        let (vec, iter) = other.get_mut();

        // Pull new items into the vector if we need them
        if vec.len() < offset {
            vec.extend(iter.as_mut().expect("no iterator?!").take(500));
        }

        // Get the token at the given offset
        let tok = if let Some(tok) = vec.get(offset) {
            Some(tok.clone())
        } else {
            None
        };

        self.0.swap(&other);

        (offset + 1, tok)
    }

    fn span(&self, range: Range<Self::Offset>) -> Self::Span {
        range.into()
    }

    fn reborrow(&self) -> Self {
        *self
    }
}

/// Represents the progress of a parser through the input
pub struct Marker<'a, I: Input<'a>> {
    pub(crate) offset: I::Offset,
    err_count: usize,
}

impl<'a, I: Input<'a>> Copy for Marker<'a, I> {}
impl<'a, I: Input<'a>> Clone for Marker<'a, I> {
    fn clone(&self) -> Self {
        *self
    }
}

/// Internal type representing an input as well as all the necessary context for parsing.
pub struct InputRef<'a, 'parse, I: Input<'a>, E: ParserExtra<'a, I>> {
    pub(crate) input: I,
    pub(crate) offset: I::Offset,
    errors: Vec<E::Error>,
    // TODO: Don't use a result, use something like `Cow` but that allows `E::State` to not be `Clone`
    state: Result<&'parse mut E::State, E::State>,
    ctx: E::Context,
    #[cfg(feature = "memoization")]
    pub(crate) memos: HashMap<(I::Offset, usize), Option<Located<E::Error>>>,
}

impl<'a, 'parse, I: Input<'a>, E: ParserExtra<'a, I>> InputRef<'a, 'parse, I, E> {
    pub(crate) fn new(input: I, state: Result<&'parse mut E::State, E::State>) -> Self
    where
        E::Context: Default,
    {
        Self {
            offset: input.start(),
            input,
            state,
            ctx: E::Context::default(),
            errors: Vec::new(),
            #[cfg(feature = "memoization")]
            memos: HashMap::default(),
        }
    }

    pub(crate) fn with_ctx<'sub_parse, CtxN, O>(
        &'sub_parse mut self,
        new_ctx: CtxN,
        f: impl FnOnce(&mut InputRef<'a, 'sub_parse, I, extra::Full<E::Error, E::State, CtxN>>) -> O,
    ) -> O
    where
        'parse: 'sub_parse,
        CtxN: 'a,
    {
        use core::mem;

        let mut new_ctx = InputRef {
            input: self.input.reborrow(),
            offset: self.offset,
            state: match &mut self.state {
                Ok(state) => Ok(*state),
                Err(state) => Ok(state),
            },
            ctx: new_ctx,
            errors: mem::take(&mut self.errors),
            #[cfg(feature = "memoization")]
            memos: HashMap::default(), // TODO: Reuse memoisation state?
        };
        let res = f(&mut new_ctx);
        self.offset = new_ctx.offset;
        self.errors = mem::take(&mut new_ctx.errors);
        res
    }

    /// Get the input offset that is currently being pointed to.
    #[inline]
    pub fn offset(&self) -> I::Offset {
        self.offset
    }

    /// Save off a [`Marker`] to the current position in the input
    #[inline]
    pub fn save(&self) -> Marker<'a, I> {
        Marker {
            offset: self.offset,
            err_count: self.errors.len(),
        }
    }

    /// Reset the input state to the provided [`Marker`]
    #[inline]
    pub fn rewind(&mut self, marker: Marker<'a, I>) {
        self.errors.truncate(marker.err_count);
        self.offset = marker.offset;
    }

    #[inline]
    pub(crate) fn state(&mut self) -> &mut E::State {
        match &mut self.state {
            Ok(state) => *state,
            Err(state) => state,
        }
    }

    #[inline]
    pub(crate) fn ctx(&self) -> &E::Context {
        &self.ctx
    }

    #[inline]
    pub(crate) fn skip_while<F: FnMut(&I::Token) -> bool>(&mut self, mut f: F) {
        let mut offs = self.offset;
        loop {
            // SAFETY: offset was generated by previous call to `Input::next`
            let (offset, token) = unsafe { self.input.next(offs) };
            if token.filter(&mut f).is_none() {
                self.offset = offs;
                break;
            } else {
                offs = offset;
            }
        }
    }

    #[inline]
    pub(crate) fn next(&mut self) -> (I::Offset, Option<I::Token>) {
        // SAFETY: offset was generated by previous call to `Input::next`
        let (offset, token) = unsafe { self.input.next(self.offset) };
        self.offset = offset;
        (self.offset, token)
    }

    #[inline]
    pub(crate) fn next_ref(&mut self) -> (I::Offset, Option<&'a I::Token>)
    where
        I: BorrowInput<'a>,
    {
        // SAFETY: offset was generated by previous call to `Input::next`
        let (offset, token) = unsafe { self.input.next_ref(self.offset) };
        self.offset = offset;
        (self.offset, token)
    }

    /// Get the next token in the input. Returns `None` for EOI
    pub fn next_token(&mut self) -> Option<I::Token> {
        self.next().1
    }

    /// Peek the next token in the input. Returns `None` for EOI
    pub fn peek(&self) -> Option<I::Token> {
        // SAFETY: offset was generated by previous call to `Input::next`
        unsafe { self.input.next(self.offset).1 }
    }

    /// Skip the next token in the input.
    #[inline]
    pub fn skip(&mut self) {
        let _ = self.next();
    }

    #[inline]
    pub(crate) fn slice(&self, range: Range<I::Offset>) -> I::Slice
    where
        I: SliceInput<'a>,
    {
        self.input.slice(range)
    }

    #[inline]
    pub(crate) fn slice_from(&self, from: RangeFrom<I::Offset>) -> I::Slice
    where
        I: SliceInput<'a>,
    {
        self.input.slice_from(from)
    }

    #[inline]
    pub(crate) fn slice_trailing(&self) -> I::Slice
    where
        I: SliceInput<'a>,
    {
        self.input.slice_from(self.offset..)
    }

    /// Return the span from the provided [`Marker`] to the current position
    #[inline]
    pub fn span_since(&self, before: I::Offset) -> I::Span {
        self.input.span(before..self.offset)
    }

    #[inline]
    #[cfg(feature = "regex")]
    pub(crate) fn skip_bytes<C>(&mut self, skip: usize)
    where
        C: Char,
        I: StrInput<'a, C>,
    {
        self.offset += skip;
    }

    #[inline]
    pub(crate) fn emit(&mut self, error: E::Error) {
        self.errors.push(error);
    }

    pub(crate) fn into_errs(self) -> Vec<E::Error> {
        self.errors
    }
}

/// Struct used in [`Parser::validate`] to collect user-emitted errors
pub struct Emitter<E> {
    emitted: Vec<E>,
}

impl<E> Emitter<E> {
    pub(crate) fn new() -> Emitter<E> {
        Emitter {
            emitted: Vec::new(),
        }
    }

    pub(crate) fn errors(self) -> Vec<E> {
        self.emitted
    }

    /// Emit a non-fatal error
    pub fn emit(&mut self, err: E) {
        self.emitted.push(err)
    }
}
