#include <fcntl.h>
#ifdef openat
#undef openat
#endif
int (*foo)(int, const char *, int, ...) = openat;
int main(void) { return 0; }
