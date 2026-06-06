#include <dirent.h>
#ifdef readdir
#undef readdir
#endif
struct dirent *(*foo)(DIR *) = readdir;
int main(void) { return 0; }
