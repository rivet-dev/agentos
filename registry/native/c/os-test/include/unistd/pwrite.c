#include <unistd.h>
#ifdef pwrite
#undef pwrite
#endif
ssize_t (*foo)(int, const void *, size_t, off_t) = pwrite;
int main(void) { return 0; }
