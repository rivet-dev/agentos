#include <stdlib.h>
#ifdef aligned_alloc
#undef aligned_alloc
#endif
void *(*foo)(size_t, size_t) = aligned_alloc;
int main(void) { return 0; }
