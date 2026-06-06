#include <dirent.h>
#ifdef opendir
#undef opendir
#endif
DIR *(*foo)(const char *) = opendir;
int main(void) { return 0; }
