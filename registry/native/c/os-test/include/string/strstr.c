#include <string.h>
#ifdef strstr
#undef strstr
#endif
char *(*foo)(const char *, const char *) = strstr;
int main(void) { return 0; }
