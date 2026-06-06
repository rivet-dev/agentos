#include <string.h>
#ifdef memmove
#undef memmove
#endif
void *(*foo)(void *, const void *, size_t) = memmove;
int main(void) { return 0; }
