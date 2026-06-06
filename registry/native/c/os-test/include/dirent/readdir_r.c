/*[OB]*/
#include <dirent.h>
#ifdef readdir_r
#undef readdir_r
#endif
int (*foo)(DIR *restrict, struct dirent *restrict, struct dirent **restrict) = readdir_r;
int main(void) { return 0; }
