#include <string.h>
#ifdef strrchr
#undef strrchr
#endif
char *(*foo)(const char *, int) = strrchr;
int main(void) { return 0; }
