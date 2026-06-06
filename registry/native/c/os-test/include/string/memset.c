#include <string.h>
#ifdef memset
#undef memset
#endif
void *(*foo)(void *, int, size_t) = memset;
int main(void) { return 0; }
