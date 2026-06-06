#include <netdb.h>
#ifdef gethostent
#undef gethostent
#endif
struct hostent *(*foo)(void) = gethostent;
int main(void) { return 0; }
