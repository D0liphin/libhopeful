#include <stddef.h>

void *lhlmalloc(size_t size, size_t align);

void *lhlfree(void *ptr);

void lhl_graph_from_root(void const *ptr, size_t size, char const *path);
