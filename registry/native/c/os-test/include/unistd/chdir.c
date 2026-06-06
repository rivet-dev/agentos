#include <unistd.h>
#ifdef chdir
#undef chdir
#endif
int (*foo)(const char *) = chdir;
int main(void) { return 0; }
