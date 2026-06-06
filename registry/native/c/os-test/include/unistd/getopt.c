#include <unistd.h>
#ifdef getopt
#undef getopt
#endif
int (*foo)(int, char *const [], const char *) = getopt;
int main(void) { return 0; }
