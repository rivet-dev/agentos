#include <dirent.h>
#ifdef closedir
#undef closedir
#endif
int (*foo)(DIR *) = closedir;
int main(void) { return 0; }
