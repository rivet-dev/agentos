#include <string.h>
#ifdef strcpy
#undef strcpy
#endif
char *(*foo)(char *restrict, const char *restrict) = strcpy;
int main(void) { return 0; }
