#include <dirent.h>
#ifdef posix_getdents
#undef posix_getdents
#endif
ssize_t (*foo)(int, void *, size_t, int) = posix_getdents;
int main(void) { return 0; }
