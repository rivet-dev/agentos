#include <unistd.h>
#ifdef readlink
#undef readlink
#endif
ssize_t (*foo)(const char *restrict, char *restrict, size_t) = readlink;
int main(void) { return 0; }
