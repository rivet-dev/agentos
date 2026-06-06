#include <string.h>
#ifdef strsignal
#undef strsignal
#endif
char *(*foo)(int) = strsignal;
int main(void) { return 0; }
