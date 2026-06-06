#include <stdlib.h>
#ifdef qsort
#undef qsort
#endif
void (*foo)(void *, size_t, size_t, int (*)(const void *, const void *)) = qsort;
int main(void) { return 0; }
