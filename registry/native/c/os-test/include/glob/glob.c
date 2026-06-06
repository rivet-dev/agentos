#include <glob.h>
#ifdef glob
#undef glob
#endif
int (*foo)(const char *restrict, int, int(*)(const char *, int), glob_t *restrict) = glob;
int main(void) { return 0; }
