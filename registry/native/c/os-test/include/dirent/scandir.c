#include <dirent.h>
#ifdef scandir
#undef scandir
#endif
int (*foo)(const char *, struct dirent ***, int (*)(const struct dirent *), int (*)(const struct dirent **, const struct dirent **)) = scandir;
int main(void) { return 0; }
