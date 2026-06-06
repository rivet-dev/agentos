#include <string.h>
#ifdef memcpy
#undef memcpy
#endif
void *(*foo)(void *restrict, const void *restrict, size_t) = memcpy;
int main(void) { return 0; }
