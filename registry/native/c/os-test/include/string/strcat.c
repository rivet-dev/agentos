#include <string.h>
#ifdef strcat
#undef strcat
#endif
char *(*foo)(char *restrict, const char *restrict) = strcat;
int main(void) { return 0; }
