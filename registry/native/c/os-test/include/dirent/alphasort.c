#include <dirent.h>
#ifdef alphasort
#undef alphasort
#endif
int (*foo)(const struct dirent **, const struct dirent **) = alphasort;
int main(void) { return 0; }
