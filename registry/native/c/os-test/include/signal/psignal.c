#include <signal.h>
#ifdef psignal
#undef psignal
#endif
void (*foo)(int, const char *) = psignal;
int main(void) { return 0; }
