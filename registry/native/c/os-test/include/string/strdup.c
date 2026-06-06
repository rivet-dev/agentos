#include <string.h>
#ifdef strdup
#undef strdup
#endif
char *(*foo)(const char *) = strdup;
int main(void) { return 0; }
