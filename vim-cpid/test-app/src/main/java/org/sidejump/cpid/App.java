package org.sidejump.cpid;

import org.junit.rules.Timeout;
import java.util.List;
import java.util.Collections;

/**
 * Hello world!
 *
 */
@SuppressWarnings
public class App
{
    public static void main( String[] args )
    {
        Timeout t = new Timeout();
        List l = new List();
        List k = Collections.of();
        System.out.println( "Hello World!" );
    }

    private <T> int returnT(@NonNull T t) {
        return t;
    }
}
