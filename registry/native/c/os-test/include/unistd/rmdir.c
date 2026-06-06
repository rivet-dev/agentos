#include <unistd.h>
#ifdef rmdir
#undef rmdir
#endif
int (*foo)(const char *) = rmdir;
int main(void) { return 0; }
