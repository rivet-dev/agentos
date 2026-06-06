#include <unistd.h>
#ifdef readlinkat
#undef readlinkat
#endif
ssize_t (*foo)(int, const char *restrict, char *restrict, size_t) = readlinkat;
int main(void) { return 0; }
