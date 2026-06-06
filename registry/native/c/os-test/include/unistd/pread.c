#include <unistd.h>
#ifdef pread
#undef pread
#endif
ssize_t (*foo)(int, void *, size_t, off_t) = pread;
int main(void) { return 0; }
