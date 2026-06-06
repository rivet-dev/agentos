#include <dirent.h>
#ifdef fdopendir
#undef fdopendir
#endif
DIR *(*foo)(int) = fdopendir;
int main(void) { return 0; }
