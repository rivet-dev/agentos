#include <stdlib.h>
#ifdef reallocarray
#undef reallocarray
#endif
void *(*foo)(void *, size_t, size_t) = reallocarray;
int main(void) { return 0; }
