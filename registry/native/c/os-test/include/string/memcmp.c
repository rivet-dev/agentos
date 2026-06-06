#include <string.h>
#ifdef memcmp
#undef memcmp
#endif
int (*foo)(const void *, const void *, size_t) = memcmp;
int main(void) { return 0; }
