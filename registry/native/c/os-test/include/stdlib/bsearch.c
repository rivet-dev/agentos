#include <stdlib.h>
#ifdef bsearch
#undef bsearch
#endif
void *(*foo)(const void *, const void *, size_t, size_t, int (*)(const void *, const void *)) = bsearch;
int main(void) { return 0; }
