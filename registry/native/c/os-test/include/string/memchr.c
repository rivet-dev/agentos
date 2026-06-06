#include <string.h>
#ifdef memchr
#undef memchr
#endif
void *(*foo)(const void *, int, size_t) = memchr;
int main(void) { return 0; }
