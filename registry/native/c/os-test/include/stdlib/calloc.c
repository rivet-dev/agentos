#include <stdlib.h>
#ifdef calloc
#undef calloc
#endif
void *(*foo)(size_t, size_t) = calloc;
int main(void) { return 0; }
