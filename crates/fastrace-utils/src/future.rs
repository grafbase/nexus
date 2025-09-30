use std::task::Poll;

use fastrace::Span;

impl<T: std::future::Future> FutureExt for T {}

pub trait FutureExt: std::future::Future + Sized {
    /// Imitating the behavior of [`fastrace::future::FutureExt::in_span()`]
    /// but returning the span alongside the output when the future completes.
    #[inline]
    fn in_span_and_out(self, span: Span) -> InSpanAndOut<Self> {
        InSpanAndOut {
            inner: self,
            span: Some(span),
        }
    }
}

#[pin_project::pin_project]
pub struct InSpanAndOut<T> {
    #[pin]
    inner: T,
    span: Option<Span>,
}

impl<T: std::future::Future> std::future::Future for InSpanAndOut<T> {
    type Output = (T::Output, Span);

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let _guard = this.span.as_ref().map(|s| s.set_local_parent());
        let res = this.inner.poll(cx);

        match res {
            Poll::Pending => Poll::Pending,
            Poll::Ready(output) => {
                let span = this
                    .span
                    .take()
                    .expect("Futures should not be polled again once they reached Ready.");
                Poll::Ready((output, span))
            }
        }
    }
}
