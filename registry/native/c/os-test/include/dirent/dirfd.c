#include <dirent.h>
#ifdef dirfd
#undef dirfd
#endif
int (*foo)(DIR *) = dirfd;
int main(void) { return 0; }
