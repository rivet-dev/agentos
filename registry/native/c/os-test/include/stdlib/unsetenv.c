#include <stdlib.h>
#ifdef unsetenv
#undef unsetenv
#endif
int (*foo)(const char *) = unsetenv;
int main(void) { return 0; }
