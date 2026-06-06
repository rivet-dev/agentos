#include <unistd.h>
#ifdef write
#undef write
#endif
ssize_t (*foo)(int, const void *, size_t) = write;
int main(void) { return 0; }
