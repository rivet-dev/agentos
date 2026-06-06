#include <string.h>
#ifdef strtok
#undef strtok
#endif
char *(*foo)(char *restrict, const char *restrict) = strtok;
int main(void) { return 0; }
