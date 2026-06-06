#include <stdlib.h>
#ifdef qsort_r
#undef qsort_r
#endif
void (*foo)(void *, size_t, size_t, int (*)(const void *, const void *, void *), void *) = qsort_r;
int main(void) { return 0; }
