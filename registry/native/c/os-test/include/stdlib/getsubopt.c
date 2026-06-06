#include <stdlib.h>
#ifdef getsubopt
#undef getsubopt
#endif
int (*foo)(char **restrict, char *const *restrict, char **restrict) = getsubopt;
int main(void) { return 0; }
