#include <string.h>
#ifdef strtok_r
#undef strtok_r
#endif
char *(*foo)(char *restrict, const char *restrict, char **restrict) = strtok_r;
int main(void) { return 0; }
