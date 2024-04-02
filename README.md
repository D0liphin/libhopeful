# libhopeful*

A simple logical heap layout visualizer. Actually the hope is for this
to be much more than just a 'logical' visualizer.

The idea is to hook into liballocs' existing allocator infrastructure. 
As a result, *hopefully* we can visualise all allocations that 
liballocs tracks. We might need to do some modifications, since I'm
guessing that the structure of liballocs is optimised for querying 
addresses as opposed to walking the whole heap.

## Things I Want

- This should be used as an executable executor. Something as simple as
  `heapvis prog`, where `prog` is a binary should produce something 
  interesting.
- This should catch at least static, malloc(), mmap() and maybe 
  stack...?
- This should stream some format that I can use from some other 
  language. I have no strong opinions about any language that might be
  my favorite or something...

## Things I Think Are Super Cool

- I like GCs... not necessarily as a forced collector for an 'overly-
  managed language'** So I want to include some kind of tracing to 
  indicate when an allocation has nothing pointing to it (that could be
  very tough).
- A GUI. This would be a complete mess to do in C, hence why I want to
  do it in Rust!

## Asterisks

*This is not necessarily going to be the name of the final project. I
just don't know what to name it yet.

**'Overly-managed' is obviously subjective. It depends entirely on the
language's goals. For example Haskell is not 'overly managed', while I
think that I would probably say Java *is*. One could argue that 
something like C or C++ is "under managed"... though I would only agree
with the latter.


