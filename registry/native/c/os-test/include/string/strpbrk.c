#include <string.h>
#ifdef strpbrk
#undef strpbrk
#endif
char *(*foo)(const char *, const char *) = strpbrk;
int main(void) { return 0; }
