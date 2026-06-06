#include <string.h>
#ifdef memmem
#undef memmem
#endif
void *(*foo)(const void *, size_t, const void *, size_t) = memmem;
int main(void) { return 0; }
