#include <stdlib.h>
#ifdef getenv
#undef getenv
#endif
char *(*foo)(const char *) = getenv;
int main(void) { return 0; }
